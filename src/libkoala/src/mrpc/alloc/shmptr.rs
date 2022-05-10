use std::ptr::NonNull;
use std::ptr::Unique;

use crate::mrpc::shared_heap::SharedHeapAllocator;
use crate::mrpc::codegen::SwitchAddressSpace;

#[derive(Debug)]
pub struct ShmPtr<T: ?Sized> {
    ptr: Unique<T>,
    ptr_remote: Unique<T>
}

impl<T: ?Sized> ShmPtr<T> {
    #[inline]
    pub fn new(ptr: *mut T) -> Option<Self> {
        if !ptr.is_null() {
            // let ptr_remote = ptr.map_addr(|addr| addr + SharedHeapAllocator::query_shm_offset(addr));
            // NOTE: unsafe
            let addr = ptr as *const () as usize;
            let metadata = std::ptr::metadata(ptr);
            let remote_addr = addr as isize + SharedHeapAllocator::query_shm_offset(addr);
            let ptr_remote = std::ptr::from_raw_parts::<T>(remote_addr as *const (), metadata).as_mut();
            let ptr = unsafe { Unique::new_unchecked(ptr) };
            let ptr_remote = unsafe { Unique::new_unchecked(ptr_remote) };
            Some(ShmPtr {
                ptr,
                ptr_remote
            })
        }
        else {
            None
        }
    }

    #[inline]
    pub unsafe fn new_unchecked(ptr: *mut T) -> Self {
       Self::new(ptr).unwrap()
    }

    #[inline]
    pub fn new_with_remote(ptr: *mut T, ptr_remote: *mut T) -> Option<Self> {
        if !ptr.is_null() && !ptr_remote.is_null() {
            let ptr = unsafe { Unique::new_unchecked(ptr) };
            let ptr_remote = unsafe { Unique::new_unchecked(ptr_remote) };
            Some(ShmPtr {
                ptr,
                ptr_remote
            })
        }
        else {
            None
        }
    }

    #[inline]
    pub unsafe fn new_unchecked_with_remote(ptr: *mut T, ptr_remote: *mut T) -> Self {
        Self::new_with_remote(ptr, ptr_remote).unwrap()
    }

    /// Acquires the underlying `*mut` pointer.
    pub fn as_ptr(self) -> *mut T {
        self.ptr.as_ptr()
    }

    /// Dereferences the content.
    ///
    /// The resulting lifetime is bound to self so this behaves "as if"
    /// it were actually an instance of T that is getting borrowed. If a longer
    /// (unbound) lifetime is needed, use `&*my_ptr.as_ptr()`.
    pub unsafe fn as_ref(&self) -> &T {
        // SAFETY: the caller must guarantee that `self` meets all the
        // requirements for a reference.
        unsafe { self.ptr.as_ref() }
    }

    /// Mutably dereferences the content.
    ///
    /// The resulting lifetime is bound to self so this behaves "as if"
    /// it were actually an instance of T that is getting borrowed. If a longer
    /// (unbound) lifetime is needed, use `&mut *my_ptr.as_ptr()`.
    #[inline]
    pub unsafe fn as_mut(&mut self) -> &mut T {
        // SAFETY: the caller must guarantee that `self` meets all the
        // requirements for a mutable reference.        
        unsafe { self.ptr.as_mut() }
    }

    /// Casts to a pointer of another type
    pub fn cast<U>(self) -> ShmPtr<U> {
        let cast_ptr = unsafe { Unique::new_unchecked(self.ptr.as_ptr() as *mut U) };
        let cast_ptr_remote = unsafe { Unique::new_unchecked(self.ptr_remote.as_ptr() as *mut U) };
        ShmPtr { ptr: cast_ptr, ptr_remote: cast_ptr_remote }
    }
}

impl<T: ?Sized> From<ShmPtr<T>> for core::ptr::NonNull<T> {
    #[inline]
    fn from(shmptr: ShmPtr<T>) -> Self {
        // SAFETY: A ShmPtr pointer cannot be null, so the conditions for
        // new_unchecked() are respected.
        unsafe { NonNull::new_unchecked(shmptr.as_ptr()) }
    }
}

impl<T: ?Sized> Clone for ShmPtr<T> {
    #[inline]
    fn clone(&self) -> Self {
        ShmPtr { ptr: self.ptr, ptr_remote: self.ptr_remote }
    }
}

impl<T: ?Sized> Copy for ShmPtr<T> {}
