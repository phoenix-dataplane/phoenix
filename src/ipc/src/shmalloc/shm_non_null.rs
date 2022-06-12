use std::fmt;

use std::ptr;
use std::ptr::NonNull;

use super::ShmPtr;

pub struct ShmNonNull<T: ?Sized> {
    // For remote pointer (ptr_backend on app side / ptr_app on backend side)
    // the following must be satisfied:
    // 1. non-deferenceable unless using unsafe
    // 2. non-zero
    // <s>3. the value must be multiple of alignment of T</s>
    // Rust does not have data types that can guarantee this property.
    // Users who deference the pointer must carefully check this condition.
    ptr_app: NonNull<T>,
    ptr_backend: NonNull<T>,
}

impl<T: Sized> ShmNonNull<T> {
    #[inline]
    pub const fn dangling() -> Self {
        ShmNonNull {
            ptr_app: NonNull::dangling(),
            ptr_backend: NonNull::dangling(),
        }
    }
}

impl<T: ?Sized> ShmNonNull<T> {
    /// # Safety: `ptr_app` and `ptr_backend` must be non-null and points to the same shared memory.
    #[must_use]
    #[inline]
    pub const unsafe fn new_unchecked(ptr_app: *mut T, ptr_backend: *mut T) -> Self {
        // SAFETY: the caller must guarantee that `ptr_app` and `ptr_backend` is non-null
        ShmNonNull {
            ptr_app: NonNull::new_unchecked(ptr_app),
            ptr_backend: NonNull::new_unchecked(ptr_backend),
        }
    }

    #[must_use]
    #[inline]
    pub const fn new(ptr_app: *mut T, ptr_backend: *mut T) -> Option<Self> {
        if !ptr_app.is_null() && !ptr_backend.is_null() {
            // SAFETY: The pointers are already checked and are not null.
            Some(unsafe { Self::new_unchecked(ptr_app, ptr_backend) })
        } else {
            None
        }
    }

    #[must_use]
    #[inline]
    pub const fn as_ptr_app(self) -> *mut T {
        self.ptr_app.as_ptr()
    }

    #[must_use]
    #[inline]
    pub const fn as_ptr_backend(self) -> *mut T {
        self.ptr_backend.as_ptr()
    }

    /// Dereferences the content on app side.
    ///
    /// The resulting lifetime is bound to self so this behaves "as if"
    /// it were actually an instance of T that is getting borrowed. If a longer
    /// (unbound) lifetime is needed, use `&*my_ptr.as_ptr_app()`.
    #[must_use]
    #[inline]
    pub const unsafe fn as_ref_app<'a>(&self) -> &'a T {
        // SAFETY: the caller must guarantee that `self` meets all the
        // requirements for a reference.
        self.ptr_app.as_ref()
    }

    /// Dereferences the content on backend side.
    #[must_use]
    #[inline]
    pub const unsafe fn as_ref_backend<'a>(&self) -> &'a T {
        // SAFETY: the caller must guarantee that `self` meets all the
        // requirements for a reference.
        self.ptr_backend.as_ref()
    }

    /// Mutably dereferences the content on app side.
    ///
    /// The resulting lifetime is bound to self so this behaves "as if"
    /// it were actually an instance of T that is getting borrowed. If a longer
    /// (unbound) lifetime is needed, use `&mut *my_ptr.as_ptr_app()`.
    #[must_use]
    #[inline]
    pub const unsafe fn as_mut_app<'a>(&mut self) -> &'a mut T {
        // SAFETY: the caller must guarantee that `self` meets all the
        // requirements for a mutable reference.
        self.ptr_app.as_mut()
    }

    /// Mutably dereferences the content on backend side.
    #[must_use]
    #[inline]
    pub const unsafe fn as_mut_backend<'a>(&mut self) -> &'a mut T {
        // SAFETY: the caller must guarantee that `self` meets all the
        // requirements for a mutable reference.
        self.ptr_backend.as_mut()
    }


    /// Casts to a pointer of another type
    #[must_use = "this returns the result of the operation, \
                  without modifying the original"]
    #[inline]
    pub const fn cast<U>(self) -> ShmNonNull<U> {
        ShmNonNull {
            ptr_app: unsafe { NonNull::new_unchecked(self.ptr_app.as_ptr() as *mut U) },
            ptr_backend: unsafe { NonNull::new_unchecked(self.ptr_backend.as_ptr() as *mut U) },
        }
    }

    /// Decompose the pointer to raw parts, ptr_app and ptr_backend
    #[must_use]
    #[inline]
    pub fn to_raw_parts(self) -> (NonNull<T>, NonNull<T>) {
        (self.ptr_app, self.ptr_backend)
    }
}

impl<T> ShmNonNull<[T]> {
    #[must_use]
    #[inline]
    pub const fn slice_from_raw_parts(
        data_app: NonNull<T>,
        data_backend: NonNull<T>,
        len: usize,
    ) -> Self {
        // SAFETY: `data` is a `NonNull` pointer which is necessarily non-null
        unsafe {
            Self::new_unchecked(
                ptr::slice_from_raw_parts_mut(data_app.as_ptr(), len),
                ptr::slice_from_raw_parts_mut(data_backend.as_ptr(), len),
            )
        }
    }

    #[must_use]
    #[inline]
    pub const fn len(self) -> usize {
        // SAFETY: The function should be safe even when called on the backend side
        // because the metadata just contains the length
        self.as_ptr_app().len()
    }

    #[inline]
    #[must_use]
    pub const fn as_non_null_ptr_app(self) -> NonNull<T> {
        // SAFETY: We know `self` is non-null.
        unsafe { NonNull::new_unchecked(self.as_ptr_app().as_mut_ptr()) }
    }

    #[inline]
    #[must_use]
    pub const fn as_non_null_ptr_backend(self) -> NonNull<T> {
        // SAFETY: We know `self` is non-null.
        unsafe { NonNull::new_unchecked(self.as_ptr_backend().as_mut_ptr()) }
    }

    #[inline]
    #[must_use]
    pub const fn as_mut_ptr_app(self) -> *mut T {
        self.as_non_null_ptr_app().as_ptr()
    }

    #[inline]
    #[must_use]
    pub const fn as_mut_ptr_backend(self) -> *mut T {
        self.as_non_null_ptr_backend().as_ptr()
    }
}


impl<T: ?Sized> From<ShmPtr<T>> for ShmNonNull<T> {
    #[inline]
    fn from(shmptr: ShmPtr<T>) -> Self {
        // SAFETY: A ShmNonNull pointer cannot be null, so the conditions for
        // new_unchecked() are respected.
        let (ptr, ptr_remote) = shmptr.to_raw_parts();
        unsafe { ShmNonNull::new_unchecked(ptr.as_ptr(), ptr_remote.as_ptr()) }
    }
}

impl<T: ?Sized> Clone for ShmNonNull<T> {
    #[inline]
    fn clone(&self) -> Self {
        ShmNonNull {
            ptr_app: self.ptr_app,
            ptr_backend: self.ptr_backend,
        }
    }
}

impl<T: ?Sized> Copy for ShmNonNull<T> {}

impl<T: ?Sized> fmt::Debug for ShmNonNull<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ShmNonNull")
         .field(&self.as_ptr_app())
         .field(&self.as_ptr_backend())
         .finish()
    }
}

impl<T: ?Sized> fmt::Pointer for ShmNonNull<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ShmNonNull")
         .field(&self.as_ptr_app())
         .field(&self.as_ptr_backend())
         .finish()
    }
}
