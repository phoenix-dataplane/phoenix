use std::fmt;
use std::marker::PhantomData;
use std::ptr::NonNull;

use super::shm_non_null::ShmNonNull;
use super::SwitchAddressSpace;

pub struct ShmPtr<T: ?Sized> {
    pointer: ShmNonNull<T>,
    _marker: PhantomData<T>,
}

/// `Unique` pointers are `Send` if `T` is `Send` because the data they
/// reference is unaliased. Note that this aliasing invariant is
/// unenforced by the type system; the abstraction using the
/// `Unique` must enforce it.
///
/// Not that for ShmPtr, the aliasing invariant must not only be enforced
/// by the abstraction using it, but also be enforced by the backend
/// (the OS service).
unsafe impl<T: Send + ?Sized> Send for ShmPtr<T> {}

/// Similar to Send.
unsafe impl<T: Sync + ?Sized> Sync for ShmPtr<T> {}

impl<T: Sized> ShmPtr<T> {
    #[must_use]
    #[inline]
    pub const fn dangling() -> Self {
        Self::from(ShmNonNull::dangling())
    }
}

impl<T: ?Sized> ShmPtr<T> {
    #[must_use]
    #[inline]
    pub const unsafe fn new_unchecked(ptr: *mut T, ptr_remote: *mut T) -> Self {
        // SAFETY: the caller must guarantee that `ptr` and `ptr_remote` is non-null.
        ShmPtr {
            pointer: ShmNonNull::new_unchecked(ptr, ptr_remote),
            _marker: PhantomData,
        }
    }

    #[must_use]
    #[inline]
    pub const fn new(ptr: *mut T, ptr_remote: *mut T) -> Option<Self> {
        if let Some(pointer) = ShmNonNull::new(ptr, ptr_remote) {
            Some(ShmPtr {
                pointer,
                _marker: PhantomData,
            })
        } else {
            None
        }
    }

    /// Acquires the underlying `*mut` pointer.
    #[must_use = "`self` will be dropped if the result is not used"]
    #[inline]
    pub fn as_ptr(self) -> *mut T {
        self.pointer.as_ptr()
    }

    // pub fn as_non_null_ptr(self) -> (NonNull<T>, usize) {
    //     (self.ptr.into(), self.addr_remote as usize)
    // }

    #[must_use]
    #[inline]
    pub fn to_raw_parts(self) -> (NonNull<T>, NonNull<T>) {
        self.pointer.to_raw_parts()
    }

    /// Dereferences the content.
    ///
    /// The resulting lifetime is bound to self so this behaves "as if"
    /// it were actually an instance of T that is getting borrowed. If a longer
    /// (unbound) lifetime is needed, use `&*my_ptr.as_ptr()`.
    #[must_use]
    #[inline]
    pub const unsafe fn as_ref(&self) -> &T {
        // SAFETY: the caller must guarantee that `self` meets all the
        // requirements for a reference.
        self.pointer.as_ref()
    }

    /// Mutably dereferences the content.
    ///
    /// The resulting lifetime is bound to self so this behaves "as if"
    /// it were actually an instance of T that is getting borrowed. If a longer
    /// (unbound) lifetime is needed, use `&mut *my_ptr.as_ptr()`.
    #[must_use]
    #[inline]
    pub const unsafe fn as_mut(&mut self) -> &mut T {
        // SAFETY: the caller must guarantee that `self` meets all the
        // requirements for a mutable reference.
        self.pointer.as_mut()
    }

    /// Casts to a pointer of another type
    pub fn cast<U>(self) -> ShmPtr<U> {
        ShmPtr::from(self.pointer.cast())
    }

    // #[inline]
    // pub fn get_remote_addr(&self) -> usize {
    //     self.addr_remote as usize
    // }
}

impl<T: ?Sized> From<ShmPtr<T>> for NonNull<T> {
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
        *self
    }
}

impl<T: ?Sized> Copy for ShmPtr<T> {}

impl<T: ?Sized> fmt::Debug for ShmPtr<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&self.as_ptr(), f)
    }
}

impl<T: ?Sized> fmt::Pointer for ShmPtr<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&self.as_ptr(), f)
    }
}

impl<T: ?Sized> const From<ShmNonNull<T>> for ShmPtr<T> {
    /// Converts a `ShmNonNull<T>` to a `ShmPtr<T>`.
    ///
    /// This conversion is infallible since `ShmNonNull` cannot be null.
    #[inline]
    fn from(pointer: ShmNonNull<T>) -> Self {
        ShmPtr {
            pointer,
            _marker: PhantomData,
        }
    }
}

unsafe impl<T: ?Sized> SwitchAddressSpace for ShmPtr<T> {
    #[inline]
    fn switch_address_space(&mut self) {
        self.pointer.switch_address_space();
    }
}