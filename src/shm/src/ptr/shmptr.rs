//! Shared memory version of std::ptr::Unique.
use std::fmt;
use std::marker::PhantomData;
use std::ptr::NonNull;

use super::shm_non_null::ShmNonNull;

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
    /// Creates a new `ShmPtr` (similar to `Unique`).
    ///
    /// # Safety
    ///
    /// `ptr_app` and `ptr_backend` must be non-null and point to the same shared memory.
    #[must_use]
    #[inline]
    pub const unsafe fn new_unchecked(ptr_app: *mut T, ptr_backend: *mut T) -> Self {
        // SAFETY: the caller must guarantee that `ptr_app` and `ptr_backend` is non-null.
        ShmPtr {
            pointer: ShmNonNull::new_unchecked(ptr_app, ptr_backend),
            _marker: PhantomData,
        }
    }

    #[must_use]
    #[inline]
    pub const fn new(ptr_app: *mut T, ptr_backend: *mut T) -> Option<Self> {
        if let Some(pointer) = ShmNonNull::new(ptr_app, ptr_backend) {
            Some(ShmPtr {
                pointer,
                _marker: PhantomData,
            })
        } else {
            None
        }
    }

    /// Acquires the underlying `*mut` pointer on app side.
    #[must_use = "`self` will be dropped if the result is not used"]
    #[inline]
    pub fn as_ptr_app(self) -> *mut T {
        self.pointer.as_ptr_app()
    }

    /// Acquires the underlying `*mut` pointer on backend side.
    #[must_use = "`self` will be dropped if the result is not used"]
    #[inline]
    pub fn as_ptr_backend(self) -> *mut T {
        self.pointer.as_ptr_backend()
    }

    #[must_use]
    #[inline]
    pub fn to_raw_parts(self) -> (NonNull<T>, NonNull<T>) {
        self.pointer.to_raw_parts()
    }

    /// Dereferences the content on app side.
    ///
    /// The resulting lifetime is bound to self so this behaves "as if"
    /// it were actually an instance of T that is getting borrowed. If a longer
    /// (unbound) lifetime is needed, use `&*my_ptr.as_ptr_app()`.
    ///
    /// # Safety
    ///
    /// When calling this method, you have to ensure that all of the following is true:
    ///
    /// * The pointer must be properly aligned.
    ///
    /// * It must be "dereferenceable" in the sense defined in [the module documentation].
    ///
    /// * You must enforce Rust's aliasing rules, since the returned lifetime `'a` is
    ///   arbitrarily chosen and does not necessarily reflect the actual lifetime of the data.
    ///   In particular, while this reference exists, the memory the pointer points to must
    ///   not get mutated (except inside `UnsafeCell`).
    ///
    /// This applies even if the result of this method is unused!
    #[must_use]
    #[inline]
    pub const unsafe fn as_ref_app(&self) -> &T {
        // SAFETY: the caller must guarantee that `self` meets all the
        // requirements for a reference.
        self.pointer.as_ref_app()
    }

    /// Dereferences the content on backend side.
    ///
    /// # Safety
    ///
    /// When calling this method, you have to ensure that all of the following is true:
    ///
    /// * The pointer must be properly aligned.
    ///
    /// * It must be "dereferenceable" in the sense defined in [the module documentation].
    ///
    /// * You must enforce Rust's aliasing rules, since the returned lifetime `'a` is
    ///   arbitrarily chosen and does not necessarily reflect the actual lifetime of the data.
    ///   In particular, while this reference exists, the memory the pointer points to must
    ///   not get mutated (except inside `UnsafeCell`).
    ///
    /// This applies even if the result of this method is unused!
    #[must_use]
    #[inline]
    pub const unsafe fn as_ref_backend(&self) -> &T {
        // SAFETY: the caller must guarantee that `self` meets all the
        // requirements for a reference.
        self.pointer.as_ref_backend()
    }

    /// Mutably dereferences the content on app side.
    ///
    /// The resulting lifetime is bound to self so this behaves "as if"
    /// it were actually an instance of T that is getting borrowed. If a longer
    /// (unbound) lifetime is needed, use `&mut *my_ptr.as_ptr()`.
    ///
    /// # Safety
    ///
    /// When calling this method, you have to ensure that all of the following is true:
    ///
    /// * The pointer must be properly aligned.
    ///
    /// * It must be "dereferenceable" in the sense defined in [the module documentation].
    ///
    /// * The pointer must point to an initialized instance of `T`.
    ///
    /// * You must enforce Rust's aliasing rules, since the returned lifetime `'a` is
    ///   arbitrarily chosen and does not necessarily reflect the actual lifetime of the data.
    ///   In particular, while this reference exists, the memory the pointer points to must
    ///   not get accessed (read or written) through any other pointer.
    ///
    /// This applies even if the result of this method is unused!
    /// (The part about being initialized is not yet fully decided, but until
    /// it is, the only safe approach is to ensure that they are indeed initialized.)
    #[must_use]
    #[inline]
    pub const unsafe fn as_mut_app(&mut self) -> &mut T {
        // SAFETY: the caller must guarantee that `self` meets all the
        // requirements for a mutable reference.
        self.pointer.as_mut_app()
    }

    /// Mutably dereferences the content on backend side.
    ///
    /// # Safety
    ///
    /// When calling this method, you have to ensure that all of the following is true:
    ///
    /// * The pointer must be properly aligned.
    ///
    /// * It must be "dereferenceable" in the sense defined in [the module documentation].
    ///
    /// * The pointer must point to an initialized instance of `T`.
    ///
    /// * You must enforce Rust's aliasing rules, since the returned lifetime `'a` is
    ///   arbitrarily chosen and does not necessarily reflect the actual lifetime of the data.
    ///   In particular, while this reference exists, the memory the pointer points to must
    ///   not get accessed (read or written) through any other pointer.
    ///
    /// This applies even if the result of this method is unused!
    /// (The part about being initialized is not yet fully decided, but until
    /// it is, the only safe approach is to ensure that they are indeed initialized.)
    #[must_use]
    #[inline]
    pub const unsafe fn as_mut_backend(&mut self) -> &mut T {
        // SAFETY: the caller must guarantee that `self` meets all the
        // requirements for a mutable reference.
        self.pointer.as_mut_backend()
    }

    /// Casts to a pointer of another type
    #[must_use]
    #[inline]
    pub const fn cast<U>(self) -> ShmPtr<U> {
        ShmPtr::from(self.pointer.cast())
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
        fmt::Debug::fmt(&self.pointer, f)
    }
}

impl<T: ?Sized> fmt::Pointer for ShmPtr<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&self.pointer, f)
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
