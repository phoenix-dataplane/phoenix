use std::alloc::LayoutError;
use std::cmp;
use std::intrinsics;
use std::mem::{self, ManuallyDrop, MaybeUninit};
use std::ops::Drop;
use std::slice;

use std::alloc::handle_alloc_error;
use std::alloc::Layout;
use std::collections::TryReserveError;
use std::collections::TryReserveErrorKind::*;

use crate::alloc::{ShmAllocator, SharedHeapAllocator};
use crate::ptr::{ShmNonNull, ShmPtr};

// use crate::salloc::SharedHeapAllocator;

use super::boxed::Box;

enum AllocInit {
    /// The contents of the new memory are uninitialized.
    Uninitialized,
    /// The new memory is guaranteed to be zeroed.
    Zeroed,
}

pub struct RawVec<T, A: ShmAllocator = SharedHeapAllocator> {
    ptr: ShmPtr<T>,
    cap: usize,
    alloc: A,
}

impl<T> RawVec<T> {
    #[must_use]
    pub const fn new() -> Self {
        Self::new_in(SharedHeapAllocator)
    }

    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self::with_capacity_in(capacity, SharedHeapAllocator)
    }

    #[inline]
    pub fn with_capacity_zeroed(capacity: usize) -> Self {
        Self::with_capacity_zeroed_in(capacity, SharedHeapAllocator)
    }
}

impl<T, A: ShmAllocator> RawVec<T, A> {
    // Tiny Vecs are dumb. Skip to:
    // - 8 if the element size is 1, because any heap allocators is likely
    //   to round up a request of less than 8 bytes to at least 8 bytes.
    // - 4 if elements are moderate-sized (<= 1 KiB).
    // - 1 otherwise, to avoid wasting too much space for very short Vecs.
    pub(crate) const MIN_NON_ZERO_CAP: usize = if mem::size_of::<T>() == 1 {
        8
    } else if mem::size_of::<T>() <= 1024 {
        4
    } else {
        1
    };

    #[must_use]
    pub const fn new_in(alloc: A) -> Self {
        Self {
            ptr: ShmPtr::dangling(),
            cap: 0,
            alloc,
        }
    }

    #[inline]
    pub fn with_capacity_in(capacity: usize, alloc: A) -> Self {
        Self::allocate_in(capacity, AllocInit::Uninitialized, alloc)
    }

    #[inline]
    pub fn with_capacity_zeroed_in(capacity: usize, alloc: A) -> Self {
        Self::allocate_in(capacity, AllocInit::Zeroed, alloc)
    }

    /// Converts the entire buffer into `Box<[MaybeUninit<T>]>` with the specified `len`.
    ///
    /// Note that this will correctly reconstitute any `cap` changes
    /// that may have been performed. (See description of type for details.)
    ///
    /// # Safety
    ///
    /// * `len` must be greater than or equal to the most recently requested capacity, and
    /// * `len` must be less than or equal to `self.capacity()`.
    ///
    /// Note, that the requested capacity and `self.capacity()` could differ, as
    /// an allocator could overallocate and return a greater memory block than requested.
    pub unsafe fn into_box(self, len: usize) -> Box<[MaybeUninit<T>]> {
        debug_assert!(
            len <= self.capacity(),
            "`len` must be smaller than or equal to `self.capacity()`"
        );

        let me = ManuallyDrop::new(self);
        let (ptr_app, ptr_backend) = me.shmptr().to_raw_parts();
        let slice_app = slice::from_raw_parts_mut(ptr_app.as_ptr() as *mut MaybeUninit<T>, len);
        let slice_backend =
            slice::from_raw_parts_mut(ptr_backend.as_ptr() as *mut MaybeUninit<T>, len);
        Box::from_raw(slice_app, slice_backend)
    }

    fn allocate_in(capacity: usize, init: AllocInit, alloc: A) -> Self {
        if mem::size_of::<T>() == 0 {
            Self::new_in(alloc)
        } else {
            let layout = match Layout::array::<T>(capacity) {
                Ok(layout) => layout,
                Err(_) => capacity_overflow(),
            };
            match alloc_guard(layout.size()) {
                Ok(_) => {}
                Err(_) => capacity_overflow(),
            }
            let result = match init {
                AllocInit::Uninitialized => alloc.allocate(layout),
                AllocInit::Zeroed => alloc.allocate_zeroed(layout),
            };
            let (ptr_app, ptr_backend) = match result {
                Ok(p) => p.to_raw_parts(),
                Err(_) => handle_alloc_error(layout),
            };
            let ptr = unsafe {
                ShmPtr::new_unchecked(ptr_app.as_mut_ptr().cast(), ptr_backend.as_mut_ptr().cast())
            };
            Self {
                ptr,
                cap: capacity,
                alloc,
            }
        }
    }

    /// Reconstitutes a `RawVec` from a pointer, capacity, and allocator.
    ///
    /// # Safety
    ///
    /// The `ptr` must be allocated (via the given allocator `alloc`), and with the given
    /// `capacity`.
    /// The `capacity` cannot exceed `isize::MAX` for sized types. (only a concern on 32-bit
    /// systems). ZST vectors may have a capacity up to `usize::MAX`.
    /// If the `ptr` and `capacity` come from a `RawVec` created via `alloc`, then this is
    /// guaranteed.
    #[inline]
    pub unsafe fn from_raw_parts_in(
        ptr_app: *mut T,
        ptr_backend: *mut T,
        capacity: usize,
        alloc: A,
    ) -> Self {
        Self {
            ptr: ShmPtr::new_unchecked(ptr_app, ptr_backend),
            cap: capacity,
            alloc,
        }
    }

    #[inline]
    pub fn ptr(&self) -> *mut T {
        self.ptr.as_ptr_app()
    }

    #[inline]
    pub fn shmptr(&self) -> ShmPtr<T> {
        self.ptr
    }

    #[inline(always)]
    pub fn capacity(&self) -> usize {
        if mem::size_of::<T>() == 0 {
            usize::MAX
        } else {
            self.cap
        }
    }

    /// Returns a shared reference to the allocator backing this `RawVec`.
    // #[inline]
    // pub fn allocator(&self) -> &A {
    //     &self.alloc
    // }

    fn current_memory(&self) -> Option<(ShmNonNull<u8>, Layout)> {
        if mem::size_of::<T>() == 0 || self.cap == 0 {
            None
        } else {
            unsafe {
                let align = mem::align_of::<T>();
                let size = mem::size_of::<T>() * self.cap;
                let layout = Layout::from_size_align_unchecked(size, align);
                let ptr = self.ptr.cast().into();
                Some((ptr, layout))
            }
        }
    }

    #[inline]
    pub fn reserve(&mut self, len: usize, additional: usize) {
        #[cold]
        fn do_reserve_and_handle<T, A: ShmAllocator>(
            slf: &mut RawVec<T, A>,
            len: usize,
            additional: usize,
        ) {
            handle_reserve(slf.grow_amortized(len, additional));
        }

        if self.needs_to_grow(len, additional) {
            do_reserve_and_handle(self, len, additional);
        }
    }

    // #[inline(never)]
    // pub fn reserve_for_push(&mut self, len: usize) {
    //     handle_reserve(self.grow_amortized(len, 1));
    // }

    /// The same as `reserve`, but returns on errors instead of panicking or aborting.
    pub fn try_reserve(&mut self, len: usize, additional: usize) -> Result<(), TryReserveError> {
        if self.needs_to_grow(len, additional) {
            self.grow_amortized(len, additional)
        } else {
            Ok(())
        }
    }

    pub fn reserve_exact(&mut self, len: usize, additional: usize) {
        handle_reserve(self.try_reserve_exact(len, additional));
    }

    pub fn try_reserve_exact(
        &mut self,
        len: usize,
        additional: usize,
    ) -> Result<(), TryReserveError> {
        if self.needs_to_grow(len, additional) {
            self.grow_exact(len, additional)
        } else {
            Ok(())
        }
    }

    pub fn shrink_to_fit(&mut self, cap: usize) {
        handle_reserve(self.shrink(cap));
    }
}

impl<T, A: ShmAllocator> RawVec<T, A> {
    fn needs_to_grow(&self, len: usize, additional: usize) -> bool {
        additional > self.capacity().wrapping_sub(len)
    }

    fn set_ptr_and_cap(&mut self, ptr: ShmNonNull<[u8]>, cap: usize) {
        let (local, remote) = ptr.to_raw_parts();
        self.ptr =
            unsafe { ShmPtr::new_unchecked(local.as_mut_ptr().cast(), remote.as_mut_ptr().cast()) };
        self.cap = cap;
    }

    fn grow_amortized(&mut self, len: usize, additional: usize) -> Result<(), TryReserveError> {
        debug_assert!(additional > 0);

        if mem::size_of::<T>() == 0 {
            return Err(CapacityOverflow.into());
        }

        let required_cap = len.checked_add(additional).ok_or(CapacityOverflow)?;

        let cap = cmp::max(self.cap * 2, required_cap);
        let cap = cmp::max(Self::MIN_NON_ZERO_CAP, cap);

        let new_layout = Layout::array::<T>(cap);

        let ptr = finish_grow(new_layout, self.current_memory(), &mut self.alloc)?;
        self.set_ptr_and_cap(ptr, cap);
        Ok(())
    }

    fn grow_exact(&mut self, len: usize, additional: usize) -> Result<(), TryReserveError> {
        if mem::size_of::<T>() == 0 {
            // Since we return a capacity of `usize::MAX` when the type size is
            // 0, getting to here necessarily means the `RawVec` is overfull.
            return Err(CapacityOverflow.into());
        }

        let cap = len.checked_add(additional).ok_or(CapacityOverflow)?;
        let new_layout = Layout::array::<T>(cap);

        // `finish_grow` is non-generic over `T`.
        let ptr = finish_grow(new_layout, self.current_memory(), &mut self.alloc)?;
        self.set_ptr_and_cap(ptr, cap);
        Ok(())
    }

    fn shrink(&mut self, cap: usize) -> Result<(), TryReserveError> {
        assert!(
            cap <= self.capacity(),
            "Tried to shrink to a larger capacity"
        );

        let (ptr, layout) = if let Some(mem) = self.current_memory() {
            mem
        } else {
            return Ok(());
        };
        let new_size = cap * mem::size_of::<T>();

        let ptr = unsafe {
            let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
            self.alloc
                .shrink(ptr, layout, new_layout)
                .map_err(|_| AllocError {
                    layout: new_layout,
                    non_exhaustive: (),
                })?
        };
        self.set_ptr_and_cap(ptr, cap);
        Ok(())
    }
}

#[inline(never)]
fn finish_grow<A: ShmAllocator>(
    new_layout: Result<Layout, LayoutError>,
    current_memory: Option<(ShmNonNull<u8>, Layout)>,
    alloc: &mut A,
) -> Result<ShmNonNull<[u8]>, TryReserveError> {
    let new_layout = new_layout.map_err(|_| CapacityOverflow)?;

    alloc_guard(new_layout.size())?;

    let memory = if let Some((ptr, old_layout)) = current_memory {
        debug_assert_eq!(old_layout.align(), new_layout.align());
        unsafe {
            // The allocator checks for alignment equality
            intrinsics::assume(old_layout.align() == new_layout.align());
            alloc.grow(ptr, old_layout, new_layout)
        }
    } else {
        alloc.allocate(new_layout)
    };

    memory.map_err(|_| {
        AllocError {
            layout: new_layout,
            non_exhaustive: (),
        }
        .into()
    })
}

impl<T, A: ShmAllocator> Drop for RawVec<T, A> {
    /// Frees the memory owned by the `RawVec` *without* trying to drop its contents.
    fn drop(&mut self) {
        if let Some((ptr, layout)) = self.current_memory() {
            // Contents (T) are dropped by RawVec's users

            // SAFETY: The `ptr` must be pointing to a previously allocated
            // location on the sender shared heap.
            self.alloc.deallocate(ptr, layout)
        }
    }
}

#[inline]
fn handle_reserve(result: Result<(), TryReserveError>) {
    match result.map_err(|e| e.kind()) {
        Err(CapacityOverflow) => capacity_overflow(),
        Err(AllocError { layout, .. }) => handle_alloc_error(layout),
        Ok(()) => { /* yay */ }
    }
}

#[inline]
fn alloc_guard(alloc_size: usize) -> Result<(), TryReserveError> {
    if usize::BITS < 64 && alloc_size > isize::MAX as usize {
        Err(CapacityOverflow.into())
    } else {
        Ok(())
    }
}

fn capacity_overflow() -> ! {
    panic!("capacity overflow");
}
