use core::any::Any;
use core::async_iter::AsyncIterator;
use core::borrow;
use core::cmp::Ordering;
use core::convert::{From, TryFrom};
use core::fmt;
use core::future::Future;
use core::hash::{Hash, Hasher};
use core::iter::FromIterator;
use core::iter::{FusedIterator, Iterator};
use core::marker::{Unpin, Unsize};
use core::mem;
use core::ops::{
    CoerceUnsized, Deref, DerefMut, DispatchFromDyn, Generator, GeneratorState, Receiver,
};
use core::pin::Pin;
use core::ptr::{self, Unique};
use core::task::{Context, Poll};

// use crate::alloc::{handle_alloc_error, WriteCloneIntoRaw};
// use crate::alloc::{AllocError, Allocator, Global, Layout};
// use crate::borrow::Cow;
// use crate::raw_vec::RawVec;
// use crate::str::from_boxed_utf8_unchecked;
// use crate::vec::Vec;

use std::alloc::{handle_alloc_error, WriteCloneIntoRaw};
use std::alloc::{AllocError, Allocator, Layout};

use crate::mrpc::shared_heap::SharedHeapAllocator;

use super::shmptr::ShmPtr;


#[fundamental]
// The declaration of the `Box` struct must be kept in sync with the
// `alloc::alloc::box_free` function or ICEs will happen. See the comment
// on `box_free` for more details.
pub struct Box<T: ?Sized>(ShmPtr<T>);

impl<T> Box<T> {
    #[inline]
    pub const fn new(x: T) -> Self
    {
        let mut boxed = Self::new_uninit();
        unsafe {
            boxed.as_mut_ptr().write(x);
            boxed.assume_init()
        }
    }

    #[inline]
    pub const fn try_new(x: T) -> Result<Self, AllocError>
    where
        T: ~const Drop,
    {
        let mut boxed = Self::try_new_uninit()?;
        unsafe {
            boxed.as_mut_ptr().write(x);
            Ok(boxed.assume_init())
        }
    }

    pub const fn new_uninit() -> Box<std::mem::MaybeUninit<T>>
    {
        let layout = Layout::new::<std::mem::MaybeUninit<T>>();
        // NOTE: Prefer match over unwrap_or_else since closure sometimes not inlineable.
        // That would make code size bigger.
        match Box::try_new_uninit() {
            Ok(m) => m,
            Err(_) => handle_alloc_error(layout),
        }
    }

    pub const fn try_new_uninit() -> Result<Box<std::mem::MaybeUninit<T>>, AllocError>
    where
    {
        let layout = Layout::new::<std::mem::MaybeUninit<T>>();
        let ptr = SharedHeapAllocator.allocate(layout)?.cast();
        unsafe { Ok(Box::from_raw(ptr.as_ptr())) }
    }

    pub const fn new_zeroed() -> Box<mem::MaybeUninit<T>>
    where
        A: ~const Allocator + ~const Drop,
    {
        let layout = Layout::new::<mem::MaybeUninit<T>>();
        // NOTE: Prefer match over unwrap_or_else since closure sometimes not inlineable.
        // That would make code size bigger.
        match Box::try_new_zeroed() {
            Ok(m) => m,
            Err(_) => handle_alloc_error(layout),
        }
    }

    pub const fn try_new_zeroed() -> Result<Box<std::mem::MaybeUninit<T>>, AllocError>
    {
        let layout = Layout::new::<std::mem::MaybeUninit<T>>();
        let ptr = SharedHeapAllocator.allocate_zeroed(layout)?.cast();
        unsafe { Ok(Box::from_raw(ptr.as_ptr())) }
    }

    #[inline(always)]
    pub const fn pin(x: T) -> Pin<Self>
    {
        Self::into_pin(Self::new(x))
    }

    /// Converts a `Box<T>` into a `Box<[T]>`
    ///
    /// This conversion does not allocate on the heap and happens in place.
    #[unstable(feature = "box_into_boxed_slice", issue = "71582")]
    #[rustc_const_unstable(feature = "const_box", issue = "92521")]
    pub const fn into_boxed_slice(boxed: Self) -> Box<[T], A> {
        let (raw, alloc) = Box::into_raw_with_allocator(boxed);
        unsafe { Box::from_raw_in(raw as *mut [T; 1], alloc) }
    }

    /// Consumes the `Box`, returning the wrapped value.
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(box_into_inner)]
    ///
    /// let c = Box::new(5);
    ///
    /// assert_eq!(Box::into_inner(c), 5);
    /// ```
    #[unstable(feature = "box_into_inner", issue = "80437")]
    #[rustc_const_unstable(feature = "const_box", issue = "92521")]
    #[inline]
    pub const fn into_inner(boxed: Self) -> T
    where
        Self: ~const Drop,
    {
        *boxed
    }
}

impl<T> Box<[T]> {
    /// Constructs a new boxed slice with uninitialized contents.
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(new_uninit)]
    ///
    /// let mut values = Box::<[u32]>::new_uninit_slice(3);
    ///
    /// let values = unsafe {
    ///     // Deferred initialization:
    ///     values[0].as_mut_ptr().write(1);
    ///     values[1].as_mut_ptr().write(2);
    ///     values[2].as_mut_ptr().write(3);
    ///
    ///     values.assume_init()
    /// };
    ///
    /// assert_eq!(*values, [1, 2, 3])
    /// ```
    #[cfg(not(no_global_oom_handling))]
    #[unstable(feature = "new_uninit", issue = "63291")]
    #[must_use]
    pub fn new_uninit_slice(len: usize) -> Box<[mem::MaybeUninit<T>]> {
        unsafe { RawVec::with_capacity(len).into_box(len) }
    }

    /// Constructs a new boxed slice with uninitialized contents, with the memory
    /// being filled with `0` bytes.
    ///
    /// See [`MaybeUninit::zeroed`][zeroed] for examples of correct and incorrect usage
    /// of this method.
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(new_uninit)]
    ///
    /// let values = Box::<[u32]>::new_zeroed_slice(3);
    /// let values = unsafe { values.assume_init() };
    ///
    /// assert_eq!(*values, [0, 0, 0])
    /// ```
    ///
    /// [zeroed]: mem::MaybeUninit::zeroed
    #[cfg(not(no_global_oom_handling))]
    #[unstable(feature = "new_uninit", issue = "63291")]
    #[must_use]
    pub fn new_zeroed_slice(len: usize) -> Box<[mem::MaybeUninit<T>]> {
        unsafe { RawVec::with_capacity_zeroed(len).into_box(len) }
    }

    /// Constructs a new boxed slice with uninitialized contents. Returns an error if
    /// the allocation fails
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(allocator_api, new_uninit)]
    ///
    /// let mut values = Box::<[u32]>::try_new_uninit_slice(3)?;
    /// let values = unsafe {
    ///     // Deferred initialization:
    ///     values[0].as_mut_ptr().write(1);
    ///     values[1].as_mut_ptr().write(2);
    ///     values[2].as_mut_ptr().write(3);
    ///     values.assume_init()
    /// };
    ///
    /// assert_eq!(*values, [1, 2, 3]);
    /// # Ok::<(), std::alloc::AllocError>(())
    /// ```
    #[unstable(feature = "allocator_api", issue = "32838")]
    #[inline]
    pub fn try_new_uninit_slice(len: usize) -> Result<Box<[mem::MaybeUninit<T>]>, AllocError> {
        unsafe {
            let layout = match Layout::array::<mem::MaybeUninit<T>>(len) {
                Ok(l) => l,
                Err(_) => return Err(AllocError),
            };
            let ptr = Global.allocate(layout)?;
            Ok(RawVec::from_raw_parts_in(ptr.as_mut_ptr() as *mut _, len, Global).into_box(len))
        }
    }

    /// Constructs a new boxed slice with uninitialized contents, with the memory
    /// being filled with `0` bytes. Returns an error if the allocation fails
    ///
    /// See [`MaybeUninit::zeroed`][zeroed] for examples of correct and incorrect usage
    /// of this method.
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(allocator_api, new_uninit)]
    ///
    /// let values = Box::<[u32]>::try_new_zeroed_slice(3)?;
    /// let values = unsafe { values.assume_init() };
    ///
    /// assert_eq!(*values, [0, 0, 0]);
    /// # Ok::<(), std::alloc::AllocError>(())
    /// ```
    ///
    /// [zeroed]: mem::MaybeUninit::zeroed
    #[inline]
    pub fn try_new_zeroed_slice(len: usize) -> Result<Box<[mem::MaybeUninit<T>]>, AllocError> {
        unsafe {
            let layout = match Layout::array::<mem::MaybeUninit<T>>(len) {
                Ok(l) => l,
                Err(_) => return Err(AllocError),
            };
            let ptr = Global.allocate_zeroed(layout)?;
            Ok(RawVec::from_raw_parts_in(ptr.as_mut_ptr() as *mut _, len, Global).into_box(len))
        }
    }
}

impl<T, A: Allocator> Box<[T], A> {
    /// Constructs a new boxed slice with uninitialized contents in the provided allocator.
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(allocator_api, new_uninit)]
    ///
    /// use std::alloc::System;
    ///
    /// let mut values = Box::<[u32], _>::new_uninit_slice_in(3, System);
    ///
    /// let values = unsafe {
    ///     // Deferred initialization:
    ///     values[0].as_mut_ptr().write(1);
    ///     values[1].as_mut_ptr().write(2);
    ///     values[2].as_mut_ptr().write(3);
    ///
    ///     values.assume_init()
    /// };
    ///
    /// assert_eq!(*values, [1, 2, 3])
    /// ```
    #[cfg(not(no_global_oom_handling))]
    #[unstable(feature = "allocator_api", issue = "32838")]
    // #[unstable(feature = "new_uninit", issue = "63291")]
    #[must_use]
    pub fn new_uninit_slice_in(len: usize, alloc: A) -> Box<[mem::MaybeUninit<T>], A> {
        unsafe { RawVec::with_capacity_in(len, alloc).into_box(len) }
    }

    /// Constructs a new boxed slice with uninitialized contents in the provided allocator,
    /// with the memory being filled with `0` bytes.
    ///
    /// See [`MaybeUninit::zeroed`][zeroed] for examples of correct and incorrect usage
    /// of this method.
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(allocator_api, new_uninit)]
    ///
    /// use std::alloc::System;
    ///
    /// let values = Box::<[u32], _>::new_zeroed_slice_in(3, System);
    /// let values = unsafe { values.assume_init() };
    ///
    /// assert_eq!(*values, [0, 0, 0])
    /// ```
    ///
    /// [zeroed]: mem::MaybeUninit::zeroed
    #[cfg(not(no_global_oom_handling))]
    #[unstable(feature = "allocator_api", issue = "32838")]
    // #[unstable(feature = "new_uninit", issue = "63291")]
    #[must_use]
    pub fn new_zeroed_slice_in(len: usize, alloc: A) -> Box<[mem::MaybeUninit<T>], A> {
        unsafe { RawVec::with_capacity_zeroed_in(len, alloc).into_box(len) }
    }
}

impl<T, A: Allocator> Box<mem::MaybeUninit<T>, A> {
    /// Converts to `Box<T, A>`.
    ///
    /// # Safety
    ///
    /// As with [`MaybeUninit::assume_init`],
    /// it is up to the caller to guarantee that the value
    /// really is in an initialized state.
    /// Calling this when the content is not yet fully initialized
    /// causes immediate undefined behavior.
    ///
    /// [`MaybeUninit::assume_init`]: mem::MaybeUninit::assume_init
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(new_uninit)]
    ///
    /// let mut five = Box::<u32>::new_uninit();
    ///
    /// let five: Box<u32> = unsafe {
    ///     // Deferred initialization:
    ///     five.as_mut_ptr().write(5);
    ///
    ///     five.assume_init()
    /// };
    ///
    /// assert_eq!(*five, 5)
    /// ```
    #[unstable(feature = "new_uninit", issue = "63291")]
    #[rustc_const_unstable(feature = "const_box", issue = "92521")]
    #[inline]
    pub const unsafe fn assume_init(self) -> Box<T, A> {
        let (raw, alloc) = Box::into_raw_with_allocator(self);
        unsafe { Box::from_raw_in(raw as *mut T, alloc) }
    }

    /// Writes the value and converts to `Box<T, A>`.
    ///
    /// This method converts the box similarly to [`Box::assume_init`] but
    /// writes `value` into it before conversion thus guaranteeing safety.
    /// In some scenarios use of this method may improve performance because
    /// the compiler may be able to optimize copying from stack.
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(new_uninit)]
    ///
    /// let big_box = Box::<[usize; 1024]>::new_uninit();
    ///
    /// let mut array = [0; 1024];
    /// for (i, place) in array.iter_mut().enumerate() {
    ///     *place = i;
    /// }
    ///
    /// // The optimizer may be able to elide this copy, so previous code writes
    /// // to heap directly.
    /// let big_box = Box::write(big_box, array);
    ///
    /// for (i, x) in big_box.iter().enumerate() {
    ///     assert_eq!(*x, i);
    /// }
    /// ```
    #[unstable(feature = "new_uninit", issue = "63291")]
    #[rustc_const_unstable(feature = "const_box", issue = "92521")]
    #[inline]
    pub const fn write(mut boxed: Self, value: T) -> Box<T, A> {
        unsafe {
            (*boxed).write(value);
            boxed.assume_init()
        }
    }
}

impl<T, A: Allocator> Box<[mem::MaybeUninit<T>], A> {
    /// Converts to `Box<[T], A>`.
    ///
    /// # Safety
    ///
    /// As with [`MaybeUninit::assume_init`],
    /// it is up to the caller to guarantee that the values
    /// really are in an initialized state.
    /// Calling this when the content is not yet fully initialized
    /// causes immediate undefined behavior.
    ///
    /// [`MaybeUninit::assume_init`]: mem::MaybeUninit::assume_init
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(new_uninit)]
    ///
    /// let mut values = Box::<[u32]>::new_uninit_slice(3);
    ///
    /// let values = unsafe {
    ///     // Deferred initialization:
    ///     values[0].as_mut_ptr().write(1);
    ///     values[1].as_mut_ptr().write(2);
    ///     values[2].as_mut_ptr().write(3);
    ///
    ///     values.assume_init()
    /// };
    ///
    /// assert_eq!(*values, [1, 2, 3])
    /// ```
    #[unstable(feature = "new_uninit", issue = "63291")]
    #[inline]
    pub unsafe fn assume_init(self) -> Box<[T], A> {
        let (raw, alloc) = Box::into_raw_with_allocator(self);
        unsafe { Box::from_raw_in(raw as *mut [T], alloc) }
    }
}

impl<T: ?Sized> Box<T> {
    #[inline]
    pub const unsafe fn from_raw(raw: *mut T) -> Self {
        Box(unsafe { ShmPtr::new_unchecked(raw) })
    }

    /// Consumes the `Box`, returning a wrapped raw pointer.
    ///
    /// The pointer will be properly aligned and non-null.
    ///
    /// After calling this function, the caller is responsible for the
    /// memory previously managed by the `Box`. In particular, the
    /// caller should properly destroy `T` and release the memory, taking
    /// into account the [memory layout] used by `Box`. The easiest way to
    /// do this is to convert the raw pointer back into a `Box` with the
    /// [`Box::from_raw`] function, allowing the `Box` destructor to perform
    /// the cleanup.
    ///
    /// Note: this is an associated function, which means that you have
    /// to call it as `Box::into_raw(b)` instead of `b.into_raw()`. This
    /// is so that there is no conflict with a method on the inner type.
    ///
    /// # Examples
    /// Converting the raw pointer back into a `Box` with [`Box::from_raw`]
    /// for automatic cleanup:
    /// ```
    /// let x = Box::new(String::from("Hello"));
    /// let ptr = Box::into_raw(x);
    /// let x = unsafe { Box::from_raw(ptr) };
    /// ```
    /// Manual cleanup by explicitly running the destructor and deallocating
    /// the memory:
    /// ```
    /// use std::alloc::{dealloc, Layout};
    /// use std::ptr;
    ///
    /// let x = Box::new(String::from("Hello"));
    /// let p = Box::into_raw(x);
    /// unsafe {
    ///     ptr::drop_in_place(p);
    ///     dealloc(p as *mut u8, Layout::new::<String>());
    /// }
    /// ```
    ///
    /// [memory layout]: self#memory-layout
    #[stable(feature = "box_raw", since = "1.4.0")]
    #[inline]
    pub fn into_raw(b: Self) -> *mut T {
        Self::into_raw_with_allocator(b).0
    }

    /// Consumes the `Box`, returning a wrapped raw pointer and the allocator.
    ///
    /// The pointer will be properly aligned and non-null.
    ///
    /// After calling this function, the caller is responsible for the
    /// memory previously managed by the `Box`. In particular, the
    /// caller should properly destroy `T` and release the memory, taking
    /// into account the [memory layout] used by `Box`. The easiest way to
    /// do this is to convert the raw pointer back into a `Box` with the
    /// [`Box::from_raw_in`] function, allowing the `Box` destructor to perform
    /// the cleanup.
    ///
    /// Note: this is an associated function, which means that you have
    /// to call it as `Box::into_raw_with_allocator(b)` instead of `b.into_raw_with_allocator()`. This
    /// is so that there is no conflict with a method on the inner type.
    ///
    /// # Examples
    /// Converting the raw pointer back into a `Box` with [`Box::from_raw_in`]
    /// for automatic cleanup:
    /// ```
    /// #![feature(allocator_api)]
    ///
    /// use std::alloc::System;
    ///
    /// let x = Box::new_in(String::from("Hello"), System);
    /// let (ptr, alloc) = Box::into_raw_with_allocator(x);
    /// let x = unsafe { Box::from_raw_in(ptr, alloc) };
    /// ```
    /// Manual cleanup by explicitly running the destructor and deallocating
    /// the memory:
    /// ```
    /// #![feature(allocator_api)]
    ///
    /// use std::alloc::{Allocator, Layout, System};
    /// use std::ptr::{self, NonNull};
    ///
    /// let x = Box::new_in(String::from("Hello"), System);
    /// let (ptr, alloc) = Box::into_raw_with_allocator(x);
    /// unsafe {
    ///     ptr::drop_in_place(ptr);
    ///     let non_null = NonNull::new_unchecked(ptr);
    ///     alloc.deallocate(non_null.cast(), Layout::new::<String>());
    /// }
    /// ```
    ///
    /// [memory layout]: self#memory-layout
    #[inline]
    pub const fn into_raw(b: Self) -> *mut T {
        let leaked = Box::into_unique(b);
        leaked.as_ptr()
    }

    #[inline]
    pub const fn into_unique(b: Self) -> Unique<T> {
        // Box is recognized as a "unique pointer" by Stacked Borrows, but internally it is a
        // raw pointer for the type system. Turning it directly into a raw pointer would not be
        // recognized as "releasing" the unique pointer to permit aliased raw accesses,
        // so all raw pointer methods have to go through `Box::leak`. Turning *that* to a raw pointer
        // behaves correctly.
        Unique::from(Box::leak(b))
    }


    #[inline]
    pub const fn into_shmptr<'a>(b: Self) -> ShmPtr<T>
    {
        unsafe { mem::ManuallyDrop::new(b).0 }
    }

    #[inline]
    pub const fn leak<'a>(b: Self) -> &'a mut T
    where
        A: 'a,
    {
        unsafe { &mut *mem::ManuallyDrop::new(b).0.as_ptr() }
    }

    pub const fn into_pin(boxed: Self) -> Pin<Self>
    where
        A: 'static,
    {
        // It's not possible to move or replace the insides of a `Pin<Box<T>>`
        // when `T: !Unpin`,  so it's safe to pin it directly without any
        // additional requirements.
        unsafe { Pin::new_unchecked(boxed) }
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
unsafe impl<#[may_dangle] T: ?Sized, A: Allocator> Drop for Box<T, A> {
    fn drop(&mut self) {
        // FIXME: Do nothing, drop is currently performed by compiler.
    }
}

#[cfg(not(no_global_oom_handling))]
#[stable(feature = "rust1", since = "1.0.0")]
impl<T: Default> Default for Box<T> {
    /// Creates a `Box<T>`, with the `Default` value for T.
    fn default() -> Self {
        box T::default()
    }
}

#[cfg(not(no_global_oom_handling))]
#[stable(feature = "rust1", since = "1.0.0")]
impl<T> Default for Box<[T]> {
    fn default() -> Self {
        Box::<[T; 0]>::new([])
    }
}

#[cfg(not(no_global_oom_handling))]
#[stable(feature = "default_box_extra", since = "1.17.0")]
impl Default for Box<str> {
    fn default() -> Self {
        unsafe { from_boxed_utf8_unchecked(Default::default()) }
    }
}

#[cfg(not(no_global_oom_handling))]
#[stable(feature = "rust1", since = "1.0.0")]
impl<T: Clone, A: Allocator + Clone> Clone for Box<T, A> {
    /// Returns a new box with a `clone()` of this box's contents.
    ///
    /// # Examples
    ///
    /// ```
    /// let x = Box::new(5);
    /// let y = x.clone();
    ///
    /// // The value is the same
    /// assert_eq!(x, y);
    ///
    /// // But they are unique objects
    /// assert_ne!(&*x as *const i32, &*y as *const i32);
    /// ```
    #[inline]
    fn clone(&self) -> Self {
        // Pre-allocate memory to allow writing the cloned value directly.
        let mut boxed = Self::new_uninit_in(self.1.clone());
        unsafe {
            (**self).write_clone_into_raw(boxed.as_mut_ptr());
            boxed.assume_init()
        }
    }

    /// Copies `source`'s contents into `self` without creating a new allocation.
    ///
    /// # Examples
    ///
    /// ```
    /// let x = Box::new(5);
    /// let mut y = Box::new(10);
    /// let yp: *const i32 = &*y;
    ///
    /// y.clone_from(&x);
    ///
    /// // The value is the same
    /// assert_eq!(x, y);
    ///
    /// // And no allocation occurred
    /// assert_eq!(yp, &*y);
    /// ```
    #[inline]
    fn clone_from(&mut self, source: &Self) {
        (**self).clone_from(&(**source));
    }
}

#[cfg(not(no_global_oom_handling))]
#[stable(feature = "box_slice_clone", since = "1.3.0")]
impl Clone for Box<str> {
    fn clone(&self) -> Self {
        // this makes a copy of the data
        let buf: Box<[u8]> = self.as_bytes().into();
        unsafe { from_boxed_utf8_unchecked(buf) }
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
impl<T: ?Sized + PartialEq, A: Allocator> PartialEq for Box<T, A> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(&**self, &**other)
    }
    #[inline]
    fn ne(&self, other: &Self) -> bool {
        PartialEq::ne(&**self, &**other)
    }
}
#[stable(feature = "rust1", since = "1.0.0")]
impl<T: ?Sized + PartialOrd, A: Allocator> PartialOrd for Box<T, A> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        PartialOrd::partial_cmp(&**self, &**other)
    }
    #[inline]
    fn lt(&self, other: &Self) -> bool {
        PartialOrd::lt(&**self, &**other)
    }
    #[inline]
    fn le(&self, other: &Self) -> bool {
        PartialOrd::le(&**self, &**other)
    }
    #[inline]
    fn ge(&self, other: &Self) -> bool {
        PartialOrd::ge(&**self, &**other)
    }
    #[inline]
    fn gt(&self, other: &Self) -> bool {
        PartialOrd::gt(&**self, &**other)
    }
}
#[stable(feature = "rust1", since = "1.0.0")]
impl<T: ?Sized + Ord, A: Allocator> Ord for Box<T, A> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        Ord::cmp(&**self, &**other)
    }
}
#[stable(feature = "rust1", since = "1.0.0")]
impl<T: ?Sized + Eq, A: Allocator> Eq for Box<T, A> {}

#[stable(feature = "rust1", since = "1.0.0")]
impl<T: ?Sized + Hash, A: Allocator> Hash for Box<T, A> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (**self).hash(state);
    }
}

#[stable(feature = "indirect_hasher_impl", since = "1.22.0")]
impl<T: ?Sized + Hasher, A: Allocator> Hasher for Box<T, A> {
    fn finish(&self) -> u64 {
        (**self).finish()
    }
    fn write(&mut self, bytes: &[u8]) {
        (**self).write(bytes)
    }
    fn write_u8(&mut self, i: u8) {
        (**self).write_u8(i)
    }
    fn write_u16(&mut self, i: u16) {
        (**self).write_u16(i)
    }
    fn write_u32(&mut self, i: u32) {
        (**self).write_u32(i)
    }
    fn write_u64(&mut self, i: u64) {
        (**self).write_u64(i)
    }
    fn write_u128(&mut self, i: u128) {
        (**self).write_u128(i)
    }
    fn write_usize(&mut self, i: usize) {
        (**self).write_usize(i)
    }
    fn write_i8(&mut self, i: i8) {
        (**self).write_i8(i)
    }
    fn write_i16(&mut self, i: i16) {
        (**self).write_i16(i)
    }
    fn write_i32(&mut self, i: i32) {
        (**self).write_i32(i)
    }
    fn write_i64(&mut self, i: i64) {
        (**self).write_i64(i)
    }
    fn write_i128(&mut self, i: i128) {
        (**self).write_i128(i)
    }
    fn write_isize(&mut self, i: isize) {
        (**self).write_isize(i)
    }
}

#[cfg(not(no_global_oom_handling))]
#[stable(feature = "from_for_ptrs", since = "1.6.0")]
impl<T> From<T> for Box<T> {
    /// Converts a `T` into a `Box<T>`
    ///
    /// The conversion allocates on the heap and moves `t`
    /// from the stack into it.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let x = 5;
    /// let boxed = Box::new(5);
    ///
    /// assert_eq!(Box::from(x), boxed);
    /// ```
    fn from(t: T) -> Self {
        Box::new(t)
    }
}

#[stable(feature = "pin", since = "1.33.0")]
#[rustc_const_unstable(feature = "const_box", issue = "92521")]
impl<T: ?Sized, A: Allocator> const From<Box<T, A>> for Pin<Box<T, A>>
where
    A: 'static,
{
    /// Converts a `Box<T>` into a `Pin<Box<T>>`
    ///
    /// This conversion does not allocate on the heap and happens in place.
    fn from(boxed: Box<T, A>) -> Self {
        Box::into_pin(boxed)
    }
}

#[cfg(not(no_global_oom_handling))]
#[stable(feature = "box_from_slice", since = "1.17.0")]
impl<T: Copy> From<&[T]> for Box<[T]> {
    /// Converts a `&[T]` into a `Box<[T]>`
    ///
    /// This conversion allocates on the heap
    /// and performs a copy of `slice`.
    ///
    /// # Examples
    /// ```rust
    /// // create a &[u8] which will be used to create a Box<[u8]>
    /// let slice: &[u8] = &[104, 101, 108, 108, 111];
    /// let boxed_slice: Box<[u8]> = Box::from(slice);
    ///
    /// println!("{:?}", boxed_slice);
    /// ```
    fn from(slice: &[T]) -> Box<[T]> {
        let len = slice.len();
        let buf = RawVec::with_capacity(len);
        unsafe {
            ptr::copy_nonoverlapping(slice.as_ptr(), buf.ptr(), len);
            buf.into_box(slice.len()).assume_init()
        }
    }
}

#[cfg(not(no_global_oom_handling))]
#[stable(feature = "box_from_cow", since = "1.45.0")]
impl<T: Copy> From<Cow<'_, [T]>> for Box<[T]> {
    /// Converts a `Cow<'_, [T]>` into a `Box<[T]>`
    ///
    /// When `cow` is the `Cow::Borrowed` variant, this
    /// conversion allocates on the heap and copies the
    /// underlying slice. Otherwise, it will try to reuse the owned
    /// `Vec`'s allocation.
    #[inline]
    fn from(cow: Cow<'_, [T]>) -> Box<[T]> {
        match cow {
            Cow::Borrowed(slice) => Box::from(slice),
            Cow::Owned(slice) => Box::from(slice),
        }
    }
}

#[cfg(not(no_global_oom_handling))]
#[stable(feature = "box_from_slice", since = "1.17.0")]
impl From<&str> for Box<str> {
    /// Converts a `&str` into a `Box<str>`
    ///
    /// This conversion allocates on the heap
    /// and performs a copy of `s`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let boxed: Box<str> = Box::from("hello");
    /// println!("{}", boxed);
    /// ```
    #[inline]
    fn from(s: &str) -> Box<str> {
        unsafe { from_boxed_utf8_unchecked(Box::from(s.as_bytes())) }
    }
}

#[cfg(not(no_global_oom_handling))]
#[stable(feature = "box_from_cow", since = "1.45.0")]
impl From<Cow<'_, str>> for Box<str> {
    /// Converts a `Cow<'_, str>` into a `Box<str>`
    ///
    /// When `cow` is the `Cow::Borrowed` variant, this
    /// conversion allocates on the heap and copies the
    /// underlying `str`. Otherwise, it will try to reuse the owned
    /// `String`'s allocation.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::borrow::Cow;
    ///
    /// let unboxed = Cow::Borrowed("hello");
    /// let boxed: Box<str> = Box::from(unboxed);
    /// println!("{}", boxed);
    /// ```
    ///
    /// ```rust
    /// # use std::borrow::Cow;
    /// let unboxed = Cow::Owned("hello".to_string());
    /// let boxed: Box<str> = Box::from(unboxed);
    /// println!("{}", boxed);
    /// ```
    #[inline]
    fn from(cow: Cow<'_, str>) -> Box<str> {
        match cow {
            Cow::Borrowed(s) => Box::from(s),
            Cow::Owned(s) => Box::from(s),
        }
    }
}

#[stable(feature = "boxed_str_conv", since = "1.19.0")]
impl<A: Allocator> From<Box<str, A>> for Box<[u8], A> {
    /// Converts a `Box<str>` into a `Box<[u8]>`
    ///
    /// This conversion does not allocate on the heap and happens in place.
    ///
    /// # Examples
    /// ```rust
    /// // create a Box<str> which will be used to create a Box<[u8]>
    /// let boxed: Box<str> = Box::from("hello");
    /// let boxed_str: Box<[u8]> = Box::from(boxed);
    ///
    /// // create a &[u8] which will be used to create a Box<[u8]>
    /// let slice: &[u8] = &[104, 101, 108, 108, 111];
    /// let boxed_slice = Box::from(slice);
    ///
    /// assert_eq!(boxed_slice, boxed_str);
    /// ```
    #[inline]
    fn from(s: Box<str, A>) -> Self {
        let (raw, alloc) = Box::into_raw_with_allocator(s);
        unsafe { Box::from_raw_in(raw as *mut [u8], alloc) }
    }
}

#[cfg(not(no_global_oom_handling))]
#[stable(feature = "box_from_array", since = "1.45.0")]
impl<T, const N: usize> From<[T; N]> for Box<[T]> {
    /// Converts a `[T; N]` into a `Box<[T]>`
    ///
    /// This conversion moves the array to newly heap-allocated memory.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let boxed: Box<[u8]> = Box::from([4, 2]);
    /// println!("{:?}", boxed);
    /// ```
    fn from(array: [T; N]) -> Box<[T]> {
        box array
    }
}

#[stable(feature = "boxed_slice_try_from", since = "1.43.0")]
impl<T, const N: usize> TryFrom<Box<[T]>> for Box<[T; N]> {
    type Error = Box<[T]>;

    /// Attempts to convert a `Box<[T]>` into a `Box<[T; N]>`.
    ///
    /// The conversion occurs in-place and does not require a
    /// new memory allocation.
    ///
    /// # Errors
    ///
    /// Returns the old `Box<[T]>` in the `Err` variant if
    /// `boxed_slice.len()` does not equal `N`.
    fn try_from(boxed_slice: Box<[T]>) -> Result<Self, Self::Error> {
        if boxed_slice.len() == N {
            Ok(unsafe { Box::from_raw(Box::into_raw(boxed_slice) as *mut [T; N]) })
        } else {
            Err(boxed_slice)
        }
    }
}

impl<A: Allocator> Box<dyn Any, A> {
    /// Attempt to downcast the box to a concrete type.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::any::Any;
    ///
    /// fn print_if_string(value: Box<dyn Any>) {
    ///     if let Ok(string) = value.downcast::<String>() {
    ///         println!("String ({}): {}", string.len(), string);
    ///     }
    /// }
    ///
    /// let my_string = "Hello World".to_string();
    /// print_if_string(Box::new(my_string));
    /// print_if_string(Box::new(0i8));
    /// ```
    #[inline]
    #[stable(feature = "rust1", since = "1.0.0")]
    pub fn downcast<T: Any>(self) -> Result<Box<T, A>, Self> {
        if self.is::<T>() { unsafe { Ok(self.downcast_unchecked::<T>()) } } else { Err(self) }
    }

    /// Downcasts the box to a concrete type.
    ///
    /// For a safe alternative see [`downcast`].
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(downcast_unchecked)]
    ///
    /// use std::any::Any;
    ///
    /// let x: Box<dyn Any> = Box::new(1_usize);
    ///
    /// unsafe {
    ///     assert_eq!(*x.downcast_unchecked::<usize>(), 1);
    /// }
    /// ```
    ///
    /// # Safety
    ///
    /// The contained value must be of type `T`. Calling this method
    /// with the incorrect type is *undefined behavior*.
    ///
    /// [`downcast`]: Self::downcast
    #[inline]
    #[unstable(feature = "downcast_unchecked", issue = "90850")]
    pub unsafe fn downcast_unchecked<T: Any>(self) -> Box<T, A> {
        debug_assert!(self.is::<T>());
        unsafe {
            let (raw, alloc): (*mut dyn Any, _) = Box::into_raw_with_allocator(self);
            Box::from_raw_in(raw as *mut T, alloc)
        }
    }
}

impl<A: Allocator> Box<dyn Any + Send, A> {
    /// Attempt to downcast the box to a concrete type.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::any::Any;
    ///
    /// fn print_if_string(value: Box<dyn Any + Send>) {
    ///     if let Ok(string) = value.downcast::<String>() {
    ///         println!("String ({}): {}", string.len(), string);
    ///     }
    /// }
    ///
    /// let my_string = "Hello World".to_string();
    /// print_if_string(Box::new(my_string));
    /// print_if_string(Box::new(0i8));
    /// ```
    #[inline]
    #[stable(feature = "rust1", since = "1.0.0")]
    pub fn downcast<T: Any>(self) -> Result<Box<T, A>, Self> {
        if self.is::<T>() { unsafe { Ok(self.downcast_unchecked::<T>()) } } else { Err(self) }
    }

    /// Downcasts the box to a concrete type.
    ///
    /// For a safe alternative see [`downcast`].
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(downcast_unchecked)]
    ///
    /// use std::any::Any;
    ///
    /// let x: Box<dyn Any + Send> = Box::new(1_usize);
    ///
    /// unsafe {
    ///     assert_eq!(*x.downcast_unchecked::<usize>(), 1);
    /// }
    /// ```
    ///
    /// # Safety
    ///
    /// The contained value must be of type `T`. Calling this method
    /// with the incorrect type is *undefined behavior*.
    ///
    /// [`downcast`]: Self::downcast
    #[inline]
    #[unstable(feature = "downcast_unchecked", issue = "90850")]
    pub unsafe fn downcast_unchecked<T: Any>(self) -> Box<T, A> {
        debug_assert!(self.is::<T>());
        unsafe {
            let (raw, alloc): (*mut (dyn Any + Send), _) = Box::into_raw_with_allocator(self);
            Box::from_raw_in(raw as *mut T, alloc)
        }
    }
}

impl<A: Allocator> Box<dyn Any + Send + Sync, A> {
    /// Attempt to downcast the box to a concrete type.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::any::Any;
    ///
    /// fn print_if_string(value: Box<dyn Any + Send + Sync>) {
    ///     if let Ok(string) = value.downcast::<String>() {
    ///         println!("String ({}): {}", string.len(), string);
    ///     }
    /// }
    ///
    /// let my_string = "Hello World".to_string();
    /// print_if_string(Box::new(my_string));
    /// print_if_string(Box::new(0i8));
    /// ```
    #[inline]
    #[stable(feature = "box_send_sync_any_downcast", since = "1.51.0")]
    pub fn downcast<T: Any>(self) -> Result<Box<T, A>, Self> {
        if self.is::<T>() { unsafe { Ok(self.downcast_unchecked::<T>()) } } else { Err(self) }
    }

    /// Downcasts the box to a concrete type.
    ///
    /// For a safe alternative see [`downcast`].
    ///
    /// # Examples
    ///
    /// ```
    /// #![feature(downcast_unchecked)]
    ///
    /// use std::any::Any;
    ///
    /// let x: Box<dyn Any + Send + Sync> = Box::new(1_usize);
    ///
    /// unsafe {
    ///     assert_eq!(*x.downcast_unchecked::<usize>(), 1);
    /// }
    /// ```
    ///
    /// # Safety
    ///
    /// The contained value must be of type `T`. Calling this method
    /// with the incorrect type is *undefined behavior*.
    ///
    /// [`downcast`]: Self::downcast
    #[inline]
    #[unstable(feature = "downcast_unchecked", issue = "90850")]
    pub unsafe fn downcast_unchecked<T: Any>(self) -> Box<T, A> {
        debug_assert!(self.is::<T>());
        unsafe {
            let (raw, alloc): (*mut (dyn Any + Send + Sync), _) =
                Box::into_raw_with_allocator(self);
            Box::from_raw_in(raw as *mut T, alloc)
        }
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
impl<T: fmt::Display + ?Sized, A: Allocator> fmt::Display for Box<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
impl<T: fmt::Debug + ?Sized, A: Allocator> fmt::Debug for Box<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
impl<T: ?Sized, A: Allocator> fmt::Pointer for Box<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // It's not possible to extract the inner Uniq directly from the Box,
        // instead we cast it to a *const which aliases the Unique
        let ptr: *const T = &**self;
        fmt::Pointer::fmt(&ptr, f)
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
#[rustc_const_unstable(feature = "const_box", issue = "92521")]
impl<T: ?Sized, A: Allocator> const Deref for Box<T, A> {
    type Target = T;

    fn deref(&self) -> &T {
        &**self
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
#[rustc_const_unstable(feature = "const_box", issue = "92521")]
impl<T: ?Sized, A: Allocator> const DerefMut for Box<T, A> {
    fn deref_mut(&mut self) -> &mut T {
        &mut **self
    }
}

#[unstable(feature = "receiver_trait", issue = "none")]
impl<T: ?Sized, A: Allocator> Receiver for Box<T, A> {}

#[stable(feature = "rust1", since = "1.0.0")]
impl<I: Iterator + ?Sized, A: Allocator> Iterator for Box<I, A> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        (**self).next()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (**self).size_hint()
    }
    fn nth(&mut self, n: usize) -> Option<I::Item> {
        (**self).nth(n)
    }
    fn last(self) -> Option<I::Item> {
        BoxIter::last(self)
    }
}

trait BoxIter {
    type Item;
    fn last(self) -> Option<Self::Item>;
}

impl<I: Iterator + ?Sized, A: Allocator> BoxIter for Box<I, A> {
    type Item = I::Item;
    default fn last(self) -> Option<I::Item> {
        #[inline]
        fn some<T>(_: Option<T>, x: T) -> Option<T> {
            Some(x)
        }

        self.fold(None, some)
    }
}

/// Specialization for sized `I`s that uses `I`s implementation of `last()`
/// instead of the default.
#[stable(feature = "rust1", since = "1.0.0")]
impl<I: Iterator, A: Allocator> BoxIter for Box<I, A> {
    fn last(self) -> Option<I::Item> {
        (*self).last()
    }
}

#[stable(feature = "rust1", since = "1.0.0")]
impl<I: DoubleEndedIterator + ?Sized, A: Allocator> DoubleEndedIterator for Box<I, A> {
    fn next_back(&mut self) -> Option<I::Item> {
        (**self).next_back()
    }
    fn nth_back(&mut self, n: usize) -> Option<I::Item> {
        (**self).nth_back(n)
    }
}
#[stable(feature = "rust1", since = "1.0.0")]
impl<I: ExactSizeIterator + ?Sized, A: Allocator> ExactSizeIterator for Box<I, A> {
    fn len(&self) -> usize {
        (**self).len()
    }
    fn is_empty(&self) -> bool {
        (**self).is_empty()
    }
}

#[stable(feature = "fused", since = "1.26.0")]
impl<I: FusedIterator + ?Sized, A: Allocator> FusedIterator for Box<I, A> {}

#[stable(feature = "boxed_closure_impls", since = "1.35.0")]
impl<Args, F: FnOnce<Args> + ?Sized, A: Allocator> FnOnce<Args> for Box<F, A> {
    type Output = <F as FnOnce<Args>>::Output;

    extern "rust-call" fn call_once(self, args: Args) -> Self::Output {
        <F as FnOnce<Args>>::call_once(*self, args)
    }
}

#[stable(feature = "boxed_closure_impls", since = "1.35.0")]
impl<Args, F: FnMut<Args> + ?Sized, A: Allocator> FnMut<Args> for Box<F, A> {
    extern "rust-call" fn call_mut(&mut self, args: Args) -> Self::Output {
        <F as FnMut<Args>>::call_mut(self, args)
    }
}

#[stable(feature = "boxed_closure_impls", since = "1.35.0")]
impl<Args, F: Fn<Args> + ?Sized, A: Allocator> Fn<Args> for Box<F, A> {
    extern "rust-call" fn call(&self, args: Args) -> Self::Output {
        <F as Fn<Args>>::call(self, args)
    }
}

#[unstable(feature = "coerce_unsized", issue = "27732")]
impl<T: ?Sized + Unsize<U>, U: ?Sized, A: Allocator> CoerceUnsized<Box<U, A>> for Box<T, A> {}

#[unstable(feature = "dispatch_from_dyn", issue = "none")]
impl<T: ?Sized + Unsize<U>, U: ?Sized> DispatchFromDyn<Box<U>> for Box<T, Global> {}

#[cfg(not(no_global_oom_handling))]
#[stable(feature = "boxed_slice_from_iter", since = "1.32.0")]
impl<I> FromIterator<I> for Box<[I]> {
    fn from_iter<T: IntoIterator<Item = I>>(iter: T) -> Self {
        iter.into_iter().collect::<Vec<_>>().into_boxed_slice()
    }
}

#[cfg(not(no_global_oom_handling))]
#[stable(feature = "box_slice_clone", since = "1.3.0")]
impl<T: Clone, A: Allocator + Clone> Clone for Box<[T], A> {
    fn clone(&self) -> Self {
        let alloc = Box::allocator(self).clone();
        self.to_vec_in(alloc).into_boxed_slice()
    }

    fn clone_from(&mut self, other: &Self) {
        if self.len() == other.len() {
            self.clone_from_slice(&other);
        } else {
            *self = other.clone();
        }
    }
}

#[stable(feature = "box_borrow", since = "1.1.0")]
impl<T: ?Sized, A: Allocator> borrow::Borrow<T> for Box<T, A> {
    fn borrow(&self) -> &T {
        &**self
    }
}

#[stable(feature = "box_borrow", since = "1.1.0")]
impl<T: ?Sized, A: Allocator> borrow::BorrowMut<T> for Box<T, A> {
    fn borrow_mut(&mut self) -> &mut T {
        &mut **self
    }
}

#[stable(since = "1.5.0", feature = "smart_ptr_as_ref")]
impl<T: ?Sized, A: Allocator> AsRef<T> for Box<T, A> {
    fn as_ref(&self) -> &T {
        &**self
    }
}

#[stable(since = "1.5.0", feature = "smart_ptr_as_ref")]
impl<T: ?Sized, A: Allocator> AsMut<T> for Box<T, A> {
    fn as_mut(&mut self) -> &mut T {
        &mut **self
    }
}

/* Nota bene
 *
 *  We could have chosen not to add this impl, and instead have written a
 *  function of Pin<Box<T>> to Pin<T>. Such a function would not be sound,
 *  because Box<T> implements Unpin even when T does not, as a result of
 *  this impl.
 *
 *  We chose this API instead of the alternative for a few reasons:
 *      - Logically, it is helpful to understand pinning in regard to the
 *        memory region being pointed to. For this reason none of the
 *        standard library pointer types support projecting through a pin
 *        (Box<T> is the only pointer type in std for which this would be
 *        safe.)
 *      - It is in practice very useful to have Box<T> be unconditionally
 *        Unpin because of trait objects, for which the structural auto
 *        trait functionality does not apply (e.g., Box<dyn Foo> would
 *        otherwise not be Unpin).
 *
 *  Another type with the same semantics as Box but only a conditional
 *  implementation of `Unpin` (where `T: Unpin`) would be valid/safe, and
 *  could have a method to project a Pin<T> from it.
 */
#[stable(feature = "pin", since = "1.33.0")]
#[rustc_const_unstable(feature = "const_box", issue = "92521")]
impl<T: ?Sized, A: Allocator> const Unpin for Box<T, A> where A: 'static {}

#[unstable(feature = "generator_trait", issue = "43122")]
impl<G: ?Sized + Generator<R> + Unpin, R, A: Allocator> Generator<R> for Box<G, A>
where
    A: 'static,
{
    type Yield = G::Yield;
    type Return = G::Return;

    fn resume(mut self: Pin<&mut Self>, arg: R) -> GeneratorState<Self::Yield, Self::Return> {
        G::resume(Pin::new(&mut *self), arg)
    }
}

#[unstable(feature = "generator_trait", issue = "43122")]
impl<G: ?Sized + Generator<R>, R, A: Allocator> Generator<R> for Pin<Box<G, A>>
where
    A: 'static,
{
    type Yield = G::Yield;
    type Return = G::Return;

    fn resume(mut self: Pin<&mut Self>, arg: R) -> GeneratorState<Self::Yield, Self::Return> {
        G::resume((*self).as_mut(), arg)
    }
}

#[stable(feature = "futures_api", since = "1.36.0")]
impl<F: ?Sized + Future + Unpin, A: Allocator> Future for Box<F, A>
where
    A: 'static,
{
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        F::poll(Pin::new(&mut *self), cx)
    }
}

#[unstable(feature = "async_iterator", issue = "79024")]
impl<S: ?Sized + AsyncIterator + Unpin> AsyncIterator for Box<S> {
    type Item = S::Item;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut **self).poll_next(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (**self).size_hint()
    }
}