use std::alloc::LayoutError;
use std::cmp;
use std::intrinsics;
use std::mem::{self, ManuallyDrop, MaybeUninit};
use std::ops::Drop;
use std::ptr::{self, NonNull, Unique};
use std::slice;

use std::alloc::handle_alloc_error;
use std::alloc::Layout;
use std::collections::TryReserveError;
use std::collections::TryReserveErrorKind::*;

use ipc::shmalloc::ShmPtr;
use ipc::shmalloc::SwitchAddressSpace;

use crate::salloc::heap::SharedHeapAllocator;
use crate::salloc::owner::{AppOwned, BackendOwned, AllocOwner};

use super::boxed::Box;

enum AllocInit {
    /// The contents of the new memory are uninitialized.
    Uninitialized,
    /// The new memory is guaranteed to be zeroed.
    Zeroed,
}

pub struct RawVec<T, O: AllocOwner = AppOwned> {
    ptr: ShmPtr<T>,
    cap: usize,
    _owner: O,
}

impl<T> RawVec<T> {
    pub(crate) const MIN_NON_ZERO_CAP: usize = if mem::size_of::<T>() == 1 {
        8
    } else if mem::size_of::<T>() <= 1024 {
        4
    } else {
        1
    };

    pub const fn new() -> Self {
        Self { ptr: ShmPtr::dangling(), cap: 0, _owner: AppOwned}
    }

    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self::allocate(capacity, AllocInit::Uninitialized)
    }

    #[inline]
    pub fn with_capacity_zeroed(capacity: usize) -> Self {
        Self::allocate(capacity, AllocInit::Zeroed)
    }

    pub unsafe fn into_box(self, len: usize) -> Box<[MaybeUninit<T>]> {
        debug_assert!(
            len <= self.capacity(),
            "`len` must be smaller than or equal to `self.capacity()`"
        );

        let me = ManuallyDrop::new(self);
        let (ptr, addr_remote) = me.ptr();
        let slice = slice::from_raw_parts_mut(ptr as *mut MaybeUninit<T>, len);
        Box::from_raw(slice, addr_remote)
    }

    fn allocate(capacity: usize, init: AllocInit) -> Self {
        if mem::size_of::<T>() == 0 {
            Self::new()
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
                AllocInit::Uninitialized => SharedHeapAllocator.allocate(layout),
                AllocInit::Zeroed => SharedHeapAllocator.allocate_zeroed(layout),
            };
            let (ptr, addr_remote) = match result {
                Ok((ptr, addr_remote)) => {
                    (ptr, addr_remote)
                },
                Err(_) => handle_alloc_error(layout),
            };

            let ptr = unsafe { ShmPtr::new_unchecked(ptr.cast().as_ptr(), addr_remote) };
            Self {
                ptr,
                cap: capacity,
                _owner: AppOwned
            }
        }
    }

    #[inline]
    pub unsafe fn from_raw_parts(ptr: *mut T, addr_remote: usize, capacity: usize) -> Self {
        Self { ptr: ShmPtr::new_unchecked(ptr, addr_remote), cap: capacity, _owner: AppOwned }
    }
}

impl<T, O: AllocOwner> RawVec<T, O> {
    #[inline]
    pub fn ptr(&self) -> (*mut T, usize) {
        self.ptr.as_ptr()
    }

    #[inline]
    pub fn shmptr(&self) -> ShmPtr<T> {
        self.ptr
    }

    #[inline(always)]
    pub fn capacity(&self) -> usize {
        if mem::size_of::<T>() == 0 { usize::MAX } else { self.cap }
    }

    fn current_memory(&self) -> Option<(NonNull<u8>, usize, Layout)> {
        if mem::size_of::<T>() == 0 || self.cap == 0 {
            None
        } else {
            unsafe {
                let align = mem::align_of::<T>();
                let size = mem::size_of::<T>() * self.cap;
                let layout = Layout::from_size_align_unchecked(size, align);
                let (ptr, addr_remote) = self.ptr.cast().as_non_null_ptr();
                Some((ptr, addr_remote, layout))
            }
        }
    }
}

impl<T> RawVec<T> {
    #[inline]
    pub fn reserve(&mut self, len: usize, additional: usize) {
        #[cold]
        fn do_reserve_and_handle<T>(
            slf: &mut RawVec<T, AppOwned>,
            len: usize,
            additional: usize,
        ) {
            handle_reserve(slf.grow_amortized(len, additional));
        }

        if self.needs_to_grow(len, additional) {
            do_reserve_and_handle(self, len, additional);
        }
    }

    #[inline(never)]
    pub fn reserve_for_push(&mut self, len: usize) {
        handle_reserve(self.grow_amortized(len, 1));
    }

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
        if self.needs_to_grow(len, additional) { self.grow_exact(len, additional) } else { Ok(()) }
    }

    pub fn shrink_to_fit(&mut self, cap: usize) {
        handle_reserve(self.shrink(cap));
    }
}

impl<T> RawVec<T> {
    fn needs_to_grow(&self, len: usize, additional: usize) -> bool {
        additional > self.capacity().wrapping_sub(len)
    }

    fn set_ptr_and_cap(&mut self, ptr: NonNull<[u8]>, addr_remote: usize, cap: usize) {
        self.ptr = unsafe { ShmPtr::new_unchecked(ptr.cast().as_ptr(), addr_remote) };
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

        let (ptr, addr_remote) = finish_grow(new_layout, self.current_memory())?;
        self.set_ptr_and_cap(ptr, addr_remote, cap);
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
        let (ptr, addr_remote) = finish_grow(new_layout, self.current_memory())?;
        self.set_ptr_and_cap(ptr, addr_remote, cap);
        Ok(())
    }

    fn shrink(&mut self, cap: usize) -> Result<(), TryReserveError> {
        assert!(cap <= self.capacity(), "Tried to shrink to a larger capacity");

        let (ptr, addr_remote, layout) = if let Some(mem) = self.current_memory() { mem } else { return Ok(()) };
        let new_size = cap * mem::size_of::<T>();

        let (ptr, addr_remote) = unsafe {
            let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
            SharedHeapAllocator
                .shrink(ptr, addr_remote, layout, new_layout)
                .map_err(|_| AllocError { layout: new_layout, non_exhaustive: () })?
        };
        self.set_ptr_and_cap(ptr, addr_remote, cap);
        Ok(())
    }
}

#[inline(never)]
fn finish_grow(
    new_layout: Result<Layout, LayoutError>,
    current_memory: Option<(NonNull<u8>, usize, Layout)>,
) -> Result<(NonNull<[u8]>, usize), TryReserveError>
{
    let new_layout = new_layout.map_err(|_| CapacityOverflow)?;

    alloc_guard(new_layout.size())?;

    let memory = if let Some((ptr, old_addr_remote, old_layout)) = current_memory {
        debug_assert_eq!(old_layout.align(), new_layout.align());
        unsafe {
            // The allocator checks for alignment equality
            intrinsics::assume(old_layout.align() == new_layout.align());
            SharedHeapAllocator.grow(ptr, old_addr_remote, old_layout, new_layout)
        }
    } else {
        SharedHeapAllocator.allocate(new_layout)
    };


    memory.map_err(|_| AllocError { layout: new_layout, non_exhaustive: () }.into())
}


trait SpecializedDrop { 
    fn drop( &mut self ); 
}

impl<T> SpecializedDrop for RawVec<T, AppOwned> {
    fn drop(&mut self) {
        if let Some((ptr, addr_remote, layout)) = self.current_memory() {
            // Contents (T) are dropped by RawVec's users
            unsafe { SharedHeapAllocator.deallocate(ptr, addr_remote, layout) }
        }
    }
}

impl<T> SpecializedDrop for RawVec<T, BackendOwned> {
    fn drop(&mut self) {
        // TODO(wyj)
        return;
    }
}

impl<T, O: AllocOwner> SpecializedDrop for RawVec<T, O> {
    default fn drop(&mut self) {
        unreachable!("SpecializedDrop should be specialized for AppOwned and BackendOwned");
    }
}


impl<T, O: AllocOwner> Drop for RawVec<T, O> {
    fn drop(&mut self) {
        (self as &mut dyn SpecializedDrop).drop();
    }
}

unsafe impl<T: SwitchAddressSpace> SwitchAddressSpace for RawVec<T> {
    fn switch_address_space(&mut self) {
        // RawVec does not handle T's switch_address_space; this is left for the users of RawVec
        self.ptr.switch_address_space();
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