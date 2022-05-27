use std::fmt;

use std::ptr;
use std::ptr::NonNull;

use super::ShmPtr;
use super::SwitchAddressSpace;

pub struct ShmNonNull<T: ?Sized> {
    ptr: NonNull<T>,
    // 1. non-deferenceable unless using unsafe
    // 2. non-zero
    // <s>3. value must be multiple of alignment of T</s>
    // Rust do not have data types that can guarantee this property.
    // User who deference the pointer needs to carefully check this condition.
    ptr_remote: NonNull<T>,
}

impl<T: Sized> ShmNonNull<T> {
    #[inline]
    pub const fn dangling() -> Self {
        ShmNonNull {
            ptr: NonNull::dangling(),
            ptr_remote: NonNull::dangling(),
        }
    }
}

impl<T: ?Sized> ShmNonNull<T> {
    #[must_use]
    #[inline]
    pub const unsafe fn new_unchecked(ptr: *mut T, ptr_remote: *mut T) -> Self {
        // SAFETY: the caller must guarantee that `ptr` and `ptr_remote` is non-null
        ShmNonNull {
            ptr: NonNull::new_unchecked(ptr),
            ptr_remote: NonNull::new_unchecked(ptr_remote),
        }
    }

    #[must_use]
    #[inline]
    pub const fn new(ptr: *mut T, ptr_remote: *mut T) -> Option<Self> {
        if !ptr.is_null() && !ptr_remote.is_null() {
            // SAFETY: The pointers are already checked and are not null.
            Some(unsafe { Self::new_unchecked(ptr, ptr_remote) })
        } else {
            None
        }
    }

    #[must_use]
    #[inline]
    pub const fn as_ptr(self) -> *mut T {
        self.ptr.as_ptr()
    }

    /// Dereferences the content.
    ///
    /// The resulting lifetime is bound to self so this behaves "as if"
    /// it were actually an instance of T that is getting borrowed. If a longer
    /// (unbound) lifetime is needed, use `&*my_ptr.as_ptr()`.
    #[must_use]
    #[inline]
    pub const unsafe fn as_ref<'a>(&self) -> &'a T {
        // SAFETY: the caller must guarantee that `self` meets all the
        // requirements for a reference.
        self.ptr.as_ref()
    }

    /// Mutably dereferences the content.
    ///
    /// The resulting lifetime is bound to self so this behaves "as if"
    /// it were actually an instance of T that is getting borrowed. If a longer
    /// (unbound) lifetime is needed, use `&mut *my_ptr.as_ptr()`.
    #[must_use]
    #[inline]
    pub const unsafe fn as_mut<'a>(&mut self) -> &'a mut T {
        // SAFETY: the caller must guarantee that `self` meets all the
        // requirements for a mutable reference.
        self.ptr.as_mut()
    }

    /// Casts to a pointer of another type
    #[must_use = "this returns the result of the operation, \
                  without modifying the original"]
    #[inline]
    pub const fn cast<U>(self) -> ShmNonNull<U> {
        ShmNonNull {
            ptr: unsafe { NonNull::new_unchecked(self.ptr.as_ptr() as *mut U) },
            ptr_remote: unsafe { NonNull::new_unchecked(self.ptr_remote.as_ptr() as *mut U) },
        }
    }

    #[must_use]
    #[inline]
    pub fn to_raw_parts(self) -> (NonNull<T>, NonNull<T>) {
        (self.ptr, self.ptr_remote)
    }
}

impl<T> ShmNonNull<[T]> {
    #[must_use]
    #[inline]
    pub const fn slice_from_raw_parts(
        data: NonNull<T>,
        data_remote: NonNull<T>,
        len: usize,
    ) -> Self {
        // SAFETY: `data` is a `NonNull` pointer which is necessarily non-null
        unsafe {
            Self::new_unchecked(
                ptr::slice_from_raw_parts_mut(data.as_ptr(), len),
                ptr::slice_from_raw_parts_mut(data_remote.as_ptr(), len),
            )
        }
    }
    
    #[must_use]
    #[inline]
    pub const fn len(self) -> usize {
        self.as_ptr().len()
    }

    #[inline]
    #[must_use]
    pub const fn as_non_null_ptr(self) -> NonNull<T> {
        // SAFETY: We know `self` is non-null.
        unsafe { NonNull::new_unchecked(self.as_ptr().as_mut_ptr()) }
    }

    #[inline]
    #[must_use]
    pub const fn as_mut_ptr(self) -> *mut T {
        self.as_non_null_ptr().as_ptr()
    }
}

impl<T: ?Sized> From<ShmNonNull<T>> for NonNull<T> {
    #[inline]
    fn from(shmptr: ShmNonNull<T>) -> Self {
        // SAFETY: A ShmNonNull pointer cannot be null, so the conditions for
        // new_unchecked() are respected.
        unsafe { NonNull::new_unchecked(shmptr.as_ptr()) }
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
            ptr: self.ptr,
            ptr_remote: self.ptr_remote,
        }
    }
}

impl<T: ?Sized> Copy for ShmNonNull<T> {}

impl<T: ?Sized> fmt::Debug for ShmNonNull<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&self.as_ptr(), f)
    }
}

impl<T: ?Sized> fmt::Pointer for ShmNonNull<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&self.as_ptr(), f)
    }
}

unsafe impl<T: ?Sized> SwitchAddressSpace for ShmNonNull<T> {
    #[inline]
    fn switch_address_space(&mut self) {
        // SAFETY: even though T can be dynamic sized, and swapping is supposed to keep their
        // original pointer metadata part. However, in all cases, whether T is a slice or a dyn trait,
        // the metadata on local and remote should be identical. Therefore, it is safe to just swap the
        // two members here.
        std::mem::swap(&mut self.ptr, &mut self.ptr_remote);
    }
}