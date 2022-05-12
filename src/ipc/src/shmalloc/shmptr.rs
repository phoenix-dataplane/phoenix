use std::ptr::NonNull;
use std::ptr::Unique;

use super::SwitchAddressSpace;

#[derive(Debug)]
pub struct ShmPtr<T: ?Sized> {
    ptr: Unique<T>,
    addr_remote: *const ()
}

impl<T: ?Sized> ShmPtr<T> {
    #[inline]
    pub fn new(ptr: *mut T, addr_remote: usize) -> Option<Self> {
        let addr_remote = addr_remote as *const ();
        if !ptr.is_null() && !addr_remote.is_null() {
            let ptr = unsafe { Unique::new_unchecked(ptr) };
            Some(ShmPtr {
                ptr,
                addr_remote
            })
        }
        else {
            None
        }
    }

    #[inline]
    pub unsafe fn new_unchecked(ptr: *mut T, addr_remote: usize) -> Self {
        // SAFETY: it is the user's responsbility to ensure addr_remote is valid
        let addr_remote = addr_remote as *const ();
        let ptr = unsafe { Unique::new_unchecked(ptr) };
        ShmPtr {
            ptr,
            addr_remote
        }
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
        self.ptr.as_ref()
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
        self.ptr.as_mut()
    }

    /// Casts to a pointer of another type
    pub fn cast<U>(self) -> ShmPtr<U> {
        let cast_ptr = unsafe { Unique::new_unchecked(self.ptr.as_ptr() as *mut U) };
        ShmPtr { ptr: cast_ptr, addr_remote: self.addr_remote }
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
        ShmPtr { ptr: self.ptr, addr_remote: self.addr_remote }
    }
}

impl<T: ?Sized> Copy for ShmPtr<T> {}


unsafe impl<T: ?Sized> SwitchAddressSpace for ShmPtr<T> {
    fn switch_address_space(&mut self) {
        self.p
    }
}

