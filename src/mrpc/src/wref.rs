//! An owned, writable reference on shared heap.
use std::mem;
use std::ops::Deref;
use std::sync::Arc;

use ipc::ptr::ShmNonNull;

use crate::alloc::Box as ShmBox;
use crate::stub::RpcData;

#[derive(PartialEq, Copy, Clone, Debug)]
pub struct WRefOpaqueVTable {
    /// This function will be called when the [`WRefOpaque`] gets cloned, e.g. when
    /// the [`WRef<T>`] which the [`WRefOpaque`] shadowed gets cloned.
    ///
    /// The implementation of this function must retain all resources that are
    /// required for this additional instance of a [`WRefOpaque`].
    clone: unsafe fn(*const ()) -> WRefOpaque,

    /// This function gets called when a [`WRefOpaque`] gets dropped.
    ///
    /// The implementation of this function must make sure to release any
    /// resources that are associated with this instance of a [`WRefOpaque`].
    drop: unsafe fn(*const ()),
}

impl WRefOpaqueVTable {
    /// Creates a new `WRefOpaqueVTable` from the provided `clone` and `drop` functions.
    ///
    /// # `clone`
    ///
    /// This function will be called when the [`WRefOpaque`] gets cloned, e.g. when
    /// the [`WRef<T>`] which the [`WRefOpaque`] shadowed gets cloned.
    ///
    /// The implementation of this function must retain all resources that are
    /// required for this additional instance of a [`WRefOpaque`].
    ///
    /// # `drop`
    ///
    /// This function gets called when a [`WRefOpaque`] gets dropped.
    ///
    /// The implementation of this function must make sure to release any
    /// resources that are associated with this instance of a [`WRefOpaque`].
    #[rustc_promotable]
    pub(crate) const fn new(
        clone: unsafe fn(*const ()) -> WRefOpaque,
        drop: unsafe fn(*const ()),
    ) -> Self {
        Self { clone, drop }
    }
}

/// A type erased object for WRef. The implemention is largely learned from `std::task::RawWaker`.
#[derive(Debug, PartialEq)]
pub(crate) struct WRefOpaque {
    data: *const (),
    vtable: WRefOpaqueVTable,
}

impl Unpin for WRefOpaque {}
unsafe impl Send for WRefOpaque {}
unsafe impl Sync for WRefOpaque {}

impl WRefOpaque {
    /// Creates a new `WRefOpaque` from the provided `data` pointer and `vtable`.
    #[inline]
    #[must_use]
    pub(crate) const fn new(data: *const (), vtable: WRefOpaqueVTable) -> Self {
        WRefOpaque { data, vtable }
    }

    /// Get the `data` pointer used to create this `WRefOpaque`.
    #[allow(unused)]
    #[inline]
    #[must_use]
    pub(crate) fn data(&self) -> *const () {
        self.data
    }

    /// Get the `vtable` pointer used to create this `WRefOpaque`.
    #[allow(unused)]
    #[inline]
    #[must_use]
    pub(crate) fn vtable(&self) -> &WRefOpaqueVTable {
        &self.vtable
    }

    #[inline]
    pub(crate) fn from_wref<T: RpcData>(wref: WRef<T>) -> Self {
        let data = WRef::into_raw(wref) as *const ();

        let clone_func: unsafe fn(*const ()) -> WRefOpaque = |data| {
            let wref: WRef<T> = unsafe { WRef::from_raw(data as _) };
            let cloned = WRef::<T>::clone(&wref);
            mem::forget(wref);
            Self::from_wref(cloned)
        };

        let drop_func: unsafe fn(*const ()) = |data| unsafe {
            let _ = WRef::<T>::from_raw(data as _);
        };

        let vtable = WRefOpaqueVTable::new(clone_func, drop_func);

        Self::new(data, vtable)
    }
}

impl Clone for WRefOpaque {
    fn clone(&self) -> Self {
        // SAFETY: TODO(cjr)
        unsafe { (self.vtable.clone)(self.data) }
    }
}

impl Drop for WRefOpaque {
    fn drop(&mut self) {
        // SAFETY: TODO(cjr)
        unsafe { (self.vtable.drop)(self.data) }
    }
}

// sealed trait pattern
mod sealed {
    pub trait Sealed {}
}

impl<T> sealed::Sealed for T {}

pub trait IntoWRef<T: RpcData>: sealed::Sealed {
    /// Wrap the input message `T` in a `WRef<T>`
    fn into_wref(self) -> WRef<T>;
}

impl<T: RpcData> IntoWRef<T> for T {
    fn into_wref(self) -> WRef<Self> {
        WRef::new(self)
    }
}

impl<T: RpcData> IntoWRef<T> for WRef<T> {
    fn into_wref(self) -> WRef<T> {
        self
    }
}

impl<T: RpcData> IntoWRef<T> for &WRef<T> {
    fn into_wref(self) -> WRef<T> {
        WRef::clone(self)
    }
}

// TODO(cjr): consider moving refcnt to ShmBox.
#[derive(Debug)]
pub struct WRef<T: RpcData>(Arc<ShmBox<T>>);

impl<T: RpcData> WRef<T> {
    #[inline]
    pub fn new(msg: T) -> Self {
        WRef(Arc::new(ShmBox::new(msg)))
    }

    #[inline]
    pub(crate) fn into_opaque(self) -> WRefOpaque {
        WRefOpaque::from_wref(self)
    }

    #[inline]
    pub(crate) fn into_shmptr(self) -> ShmNonNull<T> {
        let (ptr_app, ptr_backend) = ShmBox::to_raw_parts(self.0.deref());
        // SAFETY: both ptrs are non-null because they just came from ShmBox::to_raw_parts.
        unsafe { ShmNonNull::new_unchecked(ptr_app.as_ptr(), ptr_backend.as_ptr()) }
    }

    #[inline]
    fn into_raw(this: Self) -> *const ShmBox<T> {
        Arc::into_raw(this.0)
    }

    /// Constructs a `WRef<T>` from a raw pointer.
    ///
    /// The raw pointer must have been previously returned by a call to
    /// [`WRef<U>::into_raw`][into_raw] where `U` must have the same size and
    /// alignment as `T`. This is trivially true if `U` is `T`.
    /// Note that if `U` is not `T` but has the same size and alignment, this is
    /// basically like transmuting references of different types. See
    /// [`mem::transmute`][transmute] for more information on what
    /// restrictions apply in this case.
    #[inline]
    unsafe fn from_raw(ptr: *const ShmBox<T>) -> Self {
        WRef(Arc::from_raw(ptr))
    }
}

impl<T: RpcData> Clone for WRef<T> {
    fn clone(&self) -> Self {
        WRef(self.0.clone())
    }
}

impl<T: RpcData> Deref for WRef<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref()
    }
}

impl<T: RpcData> WRef<T> {
    #[inline]
    pub fn get_mut(this: &mut Self) -> Option<&mut T> {
        match Arc::get_mut(&mut this.0) {
            Some(shmbox) => Some(shmbox.as_mut()),
            None => None,
        }
    }

    #[inline]
    pub unsafe fn get_mut_unchecked(this: &mut Self) -> &mut T {
        // We are careful to *not* create a reference covering the "count" fields, as
        // this would alias with concurrent access to the reference counts (e.g. by `Weak`).
        // unsafe { &mut (*this.ptr.as_ptr()).data }
        Arc::get_mut_unchecked(&mut this.0).as_mut()
    }
}

// use std::mem::ManuallyDrop;
// use std::sync::atomic::{AtomicUsize, Ordering};
// use std::ptr::NonNull;
// use std::marker::PhantomData;
// #[derive(Debug)] #[repr(C)] struct WRefInner<T: ?Sized> { // reference counting.
//     // In addition to the cloning and dropping the object, there is another case to modify the
//     // refcnt: When we notify the backend service to fetch the object (on the shared heap), the
//     // refcnt is incremented and when the backend notifies that they have finished with the object,
//     // the refcnt is decremented.
//     //
//     // We do not maintain weak count because WRef type will never refer to each other.
//
//     refcnt: AtomicUsize,
//     // TODO(cjr): consider moving refcnt to ShmBox.
//
//     data: ShmBox<T>,
// }
//
// #[derive(Debug)]
// pub struct WRef<T: RpcData> {
//     inner: NonNull<WRefInner<T>>,
//     _marker: PhantomData<WRefInner<T>>,
// }
//
// impl<T: RpcData> WRef<T> {
//     pub fn new(msg: T) -> Self {
//         // allocate the metadata on standard heap
//         let x = Box::new(WRefInner {
//             refcnt: AtomicUsize::new(1),
//             data: ShmBox::new(msg),
//         });
//         unsafe { Self::from_inner(Box::leak(x).into()) }
//     }
// }
//
// impl<T: RpcData> WRef<T> {
//     unsafe fn from_inner(ptr: NonNull<WRefInner<T>>) -> Self {
//         Self { inner: ptr, _marker: PhantomData }
//     }
//
//     unsafe fn from_ptr(ptr: *mut WRefInner<T>) -> Self {
//         unsafe { Self::from_inner(NonNull::new_unchecked(ptr)) }
//     }
// }
//
//
// impl<T: Default + RpcData> Default for WRef<T> {
//     fn default() -> Self {
//         WRef::new(T::default())
//     }
// }
//
// // Make WRef type as transparent as possible to user.
// impl<T: RpcData> From<T> for WRef<T> {
//     fn from(val: T) -> Self {
//         WRef::new(val)
//     }
// }
//
// // Make WRef type as transparent as possible to user.
// impl<T: RpcData> Deref for WRef<T> {
//     type Target = T;
//
//     fn deref(&self) -> &T {
//         self.inner.as_ref()
//     }
// }
//
// impl<T: RpcData> Drop for WRef<T> {
//     fn drop(&mut self) {
//         if self.refcnt.fetch_sub(1, Ordering::AcqRel) != 1 {
//             return;
//         }
//
//         // SAFETY: this is fine because refcnt == 1
//         let _inner = unsafe { ManuallyDrop::take(&mut self.inner) };
//     }
// }
