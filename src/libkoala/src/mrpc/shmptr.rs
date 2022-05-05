/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! This crate provides a full set of shared memory smart pointers.
//! - ShmBox<T> provides an shared heap box allocation for T, only the guest can use, the uniqueness
//! is guaranteed by the compiler and the correctness of the os-service
//! - ShmPtr<T> ShmBox::into_raw(), leaked by the guest, used by the os-servie. It is extremely
//! dangerous to dereference the pointer (because the guest is untrusted, it can do anything such
//! as invalidating the value during the usage of the pointer). For this reason, ShmPtr<T> does not
//! even implement Deref or DerefMut. To access the value, one mush clone the data and get an owned
//! value with Coa<T>.
//! Coa<T>, ShmRef<T>, Src<T> (Rc, Arc, only for trusted cases).
//!
//! Note that we do not have ShmRef because it's never safe to dereference a structure on a shared
//! memory when the other side is untrusted.

//! This crate provides a [`ShmPtr`] implementation.
//! A ShmPtr has the following characteristics:
//! - Points to a shared memory region (like Box<T, SharedHeapAllocator>)
//! - Non-null (like NonNull<T>)
//! - No alias (like Unique<T>)
//! - Deref, DerefMut to &T or &mut T (smart pointers)
//! - SwitchAddressSpace on Drop, but do not touch anything about allocator.
//! - No drop ? (for RawVec and Koala, we do not want drop), sometimes, for user data, we need sth
//! like a Box<T, SharedHeapAllocator>.
//! - Non-moveable (Pin<ShmPtrInner: !Unpin>) ? (for a user data type, we hope it is non-moveable,
//! but if it is already a pointer, there is no need to make the pointer non-moveable again)
//! - CopyOnAccess, impl ToOwned for ShmPtr;
//! impl ShmBorrow<T> for ShmBox<T>;
//!
//! For the current detailed implementation, at its core, ShmPtrInner
//! consists of a base pointer and an offset to the base pointer. The base
//! pointer points to the beginning of a shared memory region. Since the
//! same shared memory can be mapped to different addresses in different
//! processes (different processes have different address spaces), and
//! we make sure that a shared memory heap is only accessible to two
//! processes (user and koala), we thus maintain an additional value
//! to record the base address in the opposite address space (vaddr_remote).
//!
//! Unlike Box<T>, dropping an object is carried out in two stages.
//! (1) When the object is ready to send to the opposite side, SwitchAddressSpace
//! is called. It simply swaps `vaddr` with `vaddr_remote` so that it not only
//! disables further use of this object in the local process, but also makes the
//! object visible to the remote process (by sending the pointers).
//! (2) When the reply of an request is delivered up to the user application, 
//! the address of the request should be switched back underhood. The request
//! can be later normally dropped and the memory space can be recycled by
//! the shared memory allocator.
//!
//! Interestingly, SwitchAddressSpace has the exact same signature as Drop, and
//! the calling order of SwitchAddressSpace is also preciously the same as Drop.
use std::pin::Pin;
use std::marker::PhantomPinned;
use std::fmt;

use unique::Unique;

#[repr(transparent)]
pub struct ShmPtr<T: ?Sized>(Pin<ShmPtrInner<T>>);

pub struct ShmPtrInner<T: ?Sized> {
    ptr: Unique<T>,
    ptr_remote: Unique<T>,
    _marker: PhantomPinned,
}

use std::ops::{Deref, DerefMut};
impl<T: ?Sized> Deref for ShmPtrInner<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        // This is safe only when ptr is managed by Allocator.
        unsafe { self.ptr.as_ref() }
    }
}

impl<T: ?Sized> DerefMut for ShmPtrInner<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.ptr.as_mut() }
    }
}

unsafe impl<T: Send + ?Sized> Send for ShmPtr<T> {}

unsafe impl<T: Sync + ?Sized> Sync for ShmPtr<T> {}

impl<T: ?Sized> ShmPtr<T> {
    /// Creates a new `Unique`.
    ///
    /// # Safety
    ///
    /// `ptr` must be non-null.
    #[inline]
    pub const unsafe fn new_unchecked(ptr: *mut T) -> Self {
        // SAFETY: the caller must guarantee that `ptr` is non-null.
        // ShmPtrInner(NonNull::new_unchecked(ptr), PhantomData)
        todo!()
        // ShmPtr(Pin::new(ShmPtrInner {
        //     ptr,
        //     
        // }))
    }

    pub const fn new(x: T) -> Self {
    }

    /// Creates a new `Unique` if `ptr` is non-null.
    #[inline]
    pub fn new(ptr: *mut T) -> Option<Self> {
        if !ptr.is_null() {
            // SAFETY: The pointer has already been checked and is not null.
            Some(unsafe { Self::new_unchecked(ptr) })
        } else {
            None
        }
    }

    /// Acquires the underlying `*mut` pointer.
    #[inline]
    pub const fn as_ptr(self) -> *mut T {
        // self.0.as_ptr()
        // self.0.as_ref().get_ref().ptr.as_ptr()
        todo!()
    }

    /// Dereferences the content.
    ///
    /// The resulting lifetime is bound to self so this behaves "as if"
    /// it were actually an instance of T that is getting borrowed. If a longer
    /// (unbound) lifetime is needed, use `&*my_ptr.as_ptr()`.
    #[inline]
    pub unsafe fn as_ref(&self) -> &T {
        // SAFETY: the caller must guarantee that `self` meets all the
        // requirements for a reference.
        &*self.as_ptr()
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
        &mut *self.as_ptr()
    }

    /// Casts to a pointer of another type.
    #[inline]
    pub const fn cast<U>(self) -> Unique<U> {
        // SAFETY: Unique::new_unchecked() creates a new unique and needs
        // the given pointer to not be null.
        // Since we are passing self as a pointer, it cannot be null.
        unsafe { Unique::new_unchecked(self.as_ptr() as *mut U) }
    }
}

// impl<T: ?Sized> Clone for ShmPtr<T> {
//     #[inline]
//     fn clone(&self) -> Self {
//         *self
//     }
// }

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

// impl<T: ?Sized> From<&mut T> for Unique<T> {
//     #[inline]
//     fn from(reference: &mut T) -> Self {
//         // SAFETY: A mutable reference cannot be null
//         unsafe { Unique::new_unchecked(reference as _) }
//     }
// }

// impl<T: ?Sized> From<Unique<T>> for NonNull<T> {
//     #[inline]
//     fn from(unique: Unique<T>) -> Self {
//         unique.0
//     }
// }
