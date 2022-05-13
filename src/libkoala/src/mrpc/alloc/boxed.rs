use core::any::Any;
use core::borrow;
use core::cmp::Ordering;
use core::convert::From;
use core::fmt;
use core::hash::{Hash, Hasher};
use core::iter::{FusedIterator, Iterator};
use core::marker::Unpin;
use core::mem;
use core::ops::{Deref, DerefMut};
use core::pin::Pin;
use core::ptr::Unique;

// use crate::alloc::{handle_alloc_error, WriteCloneIntoRaw};
// use crate::alloc::{AllocError, Allocator, Global, Layout};
// use crate::borrow::Cow;
// use crate::raw_vec::RawVec;
// use crate::str::from_boxed_utf8_unchecked;
// use crate::vec::Vec;

use std::alloc::handle_alloc_error;
use std::alloc::{AllocError, Allocator, Layout};

use ipc::shmalloc::ShmPtr;

use crate::salloc::heap::SharedHeapAllocator;

// The declaration of the `Box` struct must be kept in sync with the
// `alloc::alloc::box_free` function or ICEs will happen. See the comment
// on `box_free` for more details.
pub struct Box<T: ?Sized>(ShmPtr<T>);

impl<T> Box<T> {
    #[inline]
    pub fn new(x: T) -> Self {
        let mut boxed = Self::new_uninit();
        unsafe {
            boxed.as_mut_ptr().write(x);
            boxed.assume_init()
        }
    }

    #[inline]
    pub fn try_new(x: T) -> Result<Self, AllocError>
    where
        T: Drop,
    {
        let mut boxed = Self::try_new_uninit()?;
        unsafe {
            boxed.as_mut_ptr().write(x);
            Ok(boxed.assume_init())
        }
    }

    pub fn new_uninit() -> Box<std::mem::MaybeUninit<T>> {
        let layout = Layout::new::<std::mem::MaybeUninit<T>>();
        // NOTE: Prefer match over unwrap_or_else since closure sometimes not inlineable.
        // That would make code size bigger.
        match Box::try_new_uninit() {
            Ok(m) => m,
            Err(_) => handle_alloc_error(layout),
        }
    }

    pub fn try_new_uninit() -> Result<Box<std::mem::MaybeUninit<T>>, AllocError>
where {
        let layout = Layout::new::<std::mem::MaybeUninit<T>>();
        let ptr = SharedHeapAllocator.allocate(layout)?.cast();
        unsafe { Ok(Box::from_raw_query_remote(ptr.as_ptr())) }
    }

    pub fn new_zeroed() -> Box<mem::MaybeUninit<T>> {
        let layout = Layout::new::<mem::MaybeUninit<T>>();
        // NOTE: Prefer match over unwrap_or_else since closure sometimes not inlineable.
        // That would make code size bigger.
        match Box::try_new_zeroed() {
            Ok(m) => m,
            Err(_) => handle_alloc_error(layout),
        }
    }

    pub fn try_new_zeroed() -> Result<Box<std::mem::MaybeUninit<T>>, AllocError> {
        let layout = Layout::new::<std::mem::MaybeUninit<T>>();
        let ptr = SharedHeapAllocator.allocate_zeroed(layout)?.cast();
        unsafe { Ok(Box::from_raw_query_remote(ptr.as_ptr())) }
    }

    #[inline(always)]
    pub fn pin(x: T) -> Pin<Self> {
        Self::into_pin(Self::new(x))
    }

    pub fn into_boxed_slice(boxed: Self) -> Box<[T]> {
        let raw = Box::into_raw(boxed).0;
        unsafe { Box::from_raw_query_remote(raw as *mut [T; 1]) }
    }

    #[inline]
    pub fn into_inner(boxed: Self) -> T
    where
        Self: Drop,
    {
        let mut dst = std::mem::MaybeUninit::uninit();
        let src = Self::into_raw(boxed).0;
        unsafe {
            // copy the content from shared heap to stack
            std::ptr::copy_nonoverlapping(src.as_const(), dst.as_mut_ptr(), 1);
            // drop the content on the shared heap
            let size = core::intrinsics::size_of_val(src.as_const());
            let align = core::intrinsics::min_align_of_val(src.as_const());
            std::ptr::drop_in_place(src);
            let non_null: std::ptr::NonNull<T> = std::ptr::NonNull::new_unchecked(src);
            let layout = Layout::from_size_align_unchecked(size, align);
            SharedHeapAllocator.deallocate(non_null.cast(), layout);
            dst.assume_init()
        }
    }

    #[inline]
    pub fn unbox(self) -> T {
        Self::into_inner(self)
    }
}

// impl<T> Box<[T]> {
//     /// Constructs a new boxed slice with uninitialized contents.
//     ///
//     /// # Examples
//     ///
//     /// ```
//     /// #![feature(new_uninit)]
//     ///
//     /// let mut values = Box::<[u32]>::new_uninit_slice(3);
//     ///
//     /// let values = unsafe {
//     ///     // Deferred initialization:
//     ///     values[0].as_mut_ptr().write(1);
//     ///     values[1].as_mut_ptr().write(2);
//     ///     values[2].as_mut_ptr().write(3);
//     ///
//     ///     values.assume_init()
//     /// };
//     ///
//     /// assert_eq!(*values, [1, 2, 3])
//     /// ```
//     #[cfg(not(no_global_oom_handling))]
//     #[unstable(feature = "new_uninit", issue = "63291")]
//     #[must_use]
//     pub fn new_uninit_slice(len: usize) -> Box<[mem::MaybeUninit<T>]> {
//         unsafe { RawVec::with_capacity(len).into_box(len) }
//     }

//     /// Constructs a new boxed slice with uninitialized contents, with the memory
//     /// being filled with `0` bytes.
//     ///
//     /// See [`MaybeUninit::zeroed`][zeroed] for examples of correct and incorrect usage
//     /// of this method.
//     ///
//     /// # Examples
//     ///
//     /// ```
//     /// #![feature(new_uninit)]
//     ///
//     /// let values = Box::<[u32]>::new_zeroed_slice(3);
//     /// let values = unsafe { values.assume_init() };
//     ///
//     /// assert_eq!(*values, [0, 0, 0])
//     /// ```
//     ///
//     /// [zeroed]: mem::MaybeUninit::zeroed
//     #[cfg(not(no_global_oom_handling))]
//     #[unstable(feature = "new_uninit", issue = "63291")]
//     #[must_use]
//     pub fn new_zeroed_slice(len: usize) -> Box<[mem::MaybeUninit<T>]> {
//         unsafe { RawVec::with_capacity_zeroed(len).into_box(len) }
//     }

//     /// Constructs a new boxed slice with uninitialized contents. Returns an error if
//     /// the allocation fails
//     ///
//     /// # Examples
//     ///
//     /// ```
//     /// #![feature(allocator_api, new_uninit)]
//     ///
//     /// let mut values = Box::<[u32]>::try_new_uninit_slice(3)?;
//     /// let values = unsafe {
//     ///     // Deferred initialization:
//     ///     values[0].as_mut_ptr().write(1);
//     ///     values[1].as_mut_ptr().write(2);
//     ///     values[2].as_mut_ptr().write(3);
//     ///     values.assume_init()
//     /// };
//     ///
//     /// assert_eq!(*values, [1, 2, 3]);
//     /// # Ok::<(), std::alloc::AllocError>(())
//     /// ```
//     #[unstable(feature = "allocator_api", issue = "32838")]
//     #[inline]
//     pub fn try_new_uninit_slice(len: usize) -> Result<Box<[mem::MaybeUninit<T>]>, AllocError> {
//         unsafe {
//             let layout = match Layout::array::<mem::MaybeUninit<T>>(len) {
//                 Ok(l) => l,
//                 Err(_) => return Err(AllocError),
//             };
//             let ptr = Global.allocate(layout)?;
//             Ok(RawVec::from_raw_parts_in(ptr.as_mut_ptr() as *mut _, len, Global).into_box(len))
//         }
//     }

//     /// Constructs a new boxed slice with uninitialized contents, with the memory
//     /// being filled with `0` bytes. Returns an error if the allocation fails
//     ///
//     /// See [`MaybeUninit::zeroed`][zeroed] for examples of correct and incorrect usage
//     /// of this method.
//     ///
//     /// # Examples
//     ///
//     /// ```
//     /// #![feature(allocator_api, new_uninit)]
//     ///
//     /// let values = Box::<[u32]>::try_new_zeroed_slice(3)?;
//     /// let values = unsafe { values.assume_init() };
//     ///
//     /// assert_eq!(*values, [0, 0, 0]);
//     /// # Ok::<(), std::alloc::AllocError>(())
//     /// ```
//     ///
//     /// [zeroed]: mem::MaybeUninit::zeroed
//     #[inline]
//     pub fn try_new_zeroed_slice(len: usize) -> Result<Box<[mem::MaybeUninit<T>]>, AllocError> {
//         unsafe {
//             let layout = match Layout::array::<mem::MaybeUninit<T>>(len) {
//                 Ok(l) => l,
//                 Err(_) => return Err(AllocError),
//             };
//             let ptr = Global.allocate_zeroed(layout)?;
//             Ok(RawVec::from_raw_parts_in(ptr.as_mut_ptr() as *mut _, len, Global).into_box(len))
//         }
//     }
// }

// impl<T, A: Allocator> Box<[T], A> {
//     /// Constructs a new boxed slice with uninitialized contents in the provided allocator.
//     ///
//     /// # Examples
//     ///
//     /// ```
//     /// #![feature(allocator_api, new_uninit)]
//     ///
//     /// use std::alloc::System;
//     ///
//     /// let mut values = Box::<[u32], _>::new_uninit_slice_in(3, System);
//     ///
//     /// let values = unsafe {
//     ///     // Deferred initialization:
//     ///     values[0].as_mut_ptr().write(1);
//     ///     values[1].as_mut_ptr().write(2);
//     ///     values[2].as_mut_ptr().write(3);
//     ///
//     ///     values.assume_init()
//     /// };
//     ///
//     /// assert_eq!(*values, [1, 2, 3])
//     /// ```
//     #[cfg(not(no_global_oom_handling))]
//     #[unstable(feature = "allocator_api", issue = "32838")]
//     // #[unstable(feature = "new_uninit", issue = "63291")]
//     #[must_use]
//     pub fn new_uninit_slice_in(len: usize, alloc: A) -> Box<[mem::MaybeUninit<T>], A> {
//         unsafe { RawVec::with_capacity_in(len, alloc).into_box(len) }
//     }

//     /// Constructs a new boxed slice with uninitialized contents in the provided allocator,
//     /// with the memory being filled with `0` bytes.
//     ///
//     /// See [`MaybeUninit::zeroed`][zeroed] for examples of correct and incorrect usage
//     /// of this method.
//     ///
//     /// # Examples
//     ///
//     /// ```
//     /// #![feature(allocator_api, new_uninit)]
//     ///
//     /// use std::alloc::System;
//     ///
//     /// let values = Box::<[u32], _>::new_zeroed_slice_in(3, System);
//     /// let values = unsafe { values.assume_init() };
//     ///
//     /// assert_eq!(*values, [0, 0, 0])
//     /// ```
//     ///
//     /// [zeroed]: mem::MaybeUninit::zeroed
//     #[cfg(not(no_global_oom_handling))]
//     #[unstable(feature = "allocator_api", issue = "32838")]
//     // #[unstable(feature = "new_uninit", issue = "63291")]
//     #[must_use]
//     pub fn new_zeroed_slice_in(len: usize, alloc: A) -> Box<[mem::MaybeUninit<T>], A> {
//         unsafe { RawVec::with_capacity_zeroed_in(len, alloc).into_box(len) }
//     }
// }

impl<T> Box<std::mem::MaybeUninit<T>> {
    #[inline]
    pub unsafe fn assume_init(self) -> Box<T> {
        let raw = Box::into_raw(self).0;
        Box::from_raw_query_remote(raw as *mut T)
    }

    #[inline]
    pub fn write(mut boxed: Self, value: T) -> Box<T> {
        unsafe {
            (*boxed).write(value);
            boxed.assume_init()
        }
    }
}

impl<T> Box<[std::mem::MaybeUninit<T>]> {
    #[inline]
    pub unsafe fn assume_init(self) -> Box<[T]> {
        let raw = Box::into_raw(self).0;
        Box::from_raw_query_remote(raw as *mut [T])
    }
}

impl<T: ?Sized> Box<T> {
    unsafe fn from_raw_query_remote(raw: *mut T) -> Self {
        let addr = raw as *const () as usize;
        let addr_remote = SharedHeapAllocator::query_backend_addr(addr);
        Box::from_raw(raw, addr_remote)
    }
}

impl<T: ?Sized> Box<T> {
    // TODO(wyj): shouldn't drop the Box if it is createdby the backend
    #[inline]
    pub unsafe fn from_raw(raw: *mut T, addr_remote: usize) -> Self {
        Box(ShmPtr::new_unchecked(raw, addr_remote))
    }

    pub unsafe fn from_shmptr(raw: ShmPtr<T>) -> Self {
        Box(raw)
    }

    #[inline]
    pub fn into_raw(b: Self) -> (*mut T, usize) {
        let addr_remote = b.0.get_remote_addr();
        let leaked = Box::into_unique(b);
        (leaked.as_ptr(), addr_remote)
    }

    #[inline]
    pub fn into_unique(b: Self) -> Unique<T> {
        // Box is recognized as a "unique pointer" by Stacked Borrows, but internally it is a
        // raw pointer for the type system. Turning it directly into a raw pointer would not be
        // recognized as "releasing" the unique pointer to permit aliased raw accesses,
        // so all raw pointer methods have to go through `Box::leak`. Turning *that* to a raw pointer
        // behaves correctly.
        Unique::from(Box::leak(b))
    }

    #[inline]
    pub fn into_shmptr<'a>(b: Self) -> ShmPtr<T> {
        mem::ManuallyDrop::new(b).0
    }

    #[inline]
    pub fn leak<'a>(b: Self) -> &'a mut T {
        unsafe { &mut *mem::ManuallyDrop::new(b).0.as_ptr() }
    }

    pub fn into_pin(boxed: Self) -> Pin<Self> {
        // It's not possible to move or replace the insides of a `Pin<Box<T>>`
        // when `T: !Unpin`,  so it's safe to pin it directly without any
        // additional requirements.
        unsafe { Pin::new_unchecked(boxed) }
    }
}

impl<T: ?Sized> Drop for Box<T> {
    fn drop(&mut self) {
        unsafe {
            let size = core::intrinsics::size_of_val(self.0.as_ref());
            let align = core::intrinsics::min_align_of_val(self.0.as_ref());
            std::ptr::drop_in_place(self.0.as_ptr());
            let non_null: std::ptr::NonNull<T> = self.0.into();
            let layout = Layout::from_size_align_unchecked(size, align);
            SharedHeapAllocator.deallocate(non_null.cast(), layout);
        }
    }
}

impl<T: Default> Default for Box<T> {
    fn default() -> Self {
        Box::new(T::default())
    }
}

// impl<T> Default for Box<[T]> {
//     fn default() -> Self {
//         Box::<[T; 0]>::new([])
//     }
// }

#[inline]
pub(crate) unsafe fn from_boxed_utf8_unchecked(v: Box<[u8]>) -> Box<str> {
    Box::from_raw_query_remote(Box::into_raw(v).0 as *mut str)
}

// impl Default for Box<str> {
//     fn default() -> Self {
//         unsafe { from_boxed_utf8_unchecked(Default::default()) }
//     }
// }

pub(crate) trait WriteCloneIntoRaw: Sized {
    unsafe fn write_clone_into_raw(&self, target: *mut Self);
}

impl<T: Clone> WriteCloneIntoRaw for T {
    #[inline]
    default unsafe fn write_clone_into_raw(&self, target: *mut Self) {
        // Having allocated *first* may allow the optimizer to create
        // the cloned value in-place, skipping the local and move.
        target.write(self.clone());
    }
}

impl<T: Copy> WriteCloneIntoRaw for T {
    #[inline]
    unsafe fn write_clone_into_raw(&self, target: *mut Self) {
        // We can always copy in-place, without ever involving a local value.
        target.copy_from_nonoverlapping(self, 1);
    }
}

impl<T: Clone> Clone for Box<T> {
    #[inline]
    fn clone(&self) -> Self {
        // Pre-allocate memory to allow writing the cloned value directly.
        let mut boxed = Self::new_uninit();
        unsafe {
            (**self).write_clone_into_raw(boxed.as_mut_ptr());
            boxed.assume_init()
        }
    }

    #[inline]
    fn clone_from(&mut self, source: &Self) {
        (**self).clone_from(&(**source));
    }
}

// impl Clone for Box<str> {
//     fn clone(&self) -> Self {
//         // this makes a copy of the data
//         let buf: Box<[u8]> = self.as_bytes().into();
//         unsafe { from_boxed_utf8_unchecked(buf) }
//     }
// }

impl<T: ?Sized + PartialEq> PartialEq for Box<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(&**self, &**other)
    }
    #[inline]
    fn ne(&self, other: &Self) -> bool {
        PartialEq::ne(&**self, &**other)
    }
}

impl<T: ?Sized + PartialOrd> PartialOrd for Box<T> {
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

impl<T: ?Sized + Ord> Ord for Box<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        Ord::cmp(&**self, &**other)
    }
}

impl<T: ?Sized + Eq> Eq for Box<T> {}

impl<T: ?Sized + Hash> Hash for Box<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (**self).hash(state);
    }
}

impl<T: ?Sized + Hasher> Hasher for Box<T> {
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

impl<T> From<T> for Box<T> {
    fn from(t: T) -> Self {
        Box::new(t)
    }
}

impl<T: ?Sized> From<Box<T>> for Pin<Box<T>> {
    fn from(boxed: Box<T>) -> Self {
        Box::into_pin(boxed)
    }
}

// #[cfg(not(no_global_oom_handling))]
// #[stable(feature = "box_from_slice", since = "1.17.0")]
// impl<T: Copy> From<&[T]> for Box<[T]> {
//     /// Converts a `&[T]` into a `Box<[T]>`
//     ///
//     /// This conversion allocates on the heap
//     /// and performs a copy of `slice`.
//     ///
//     /// # Examples
//     /// ```rust
//     /// // create a &[u8] which will be used to create a Box<[u8]>
//     /// let slice: &[u8] = &[104, 101, 108, 108, 111];
//     /// let boxed_slice: Box<[u8]> = Box::from(slice);
//     ///
//     /// println!("{:?}", boxed_slice);
//     /// ```
//     fn from(slice: &[T]) -> Box<[T]> {
//         let len = slice.len();
//         let buf = RawVec::with_capacity(len);
//         unsafe {
//             ptr::copy_nonoverlapping(slice.as_ptr(), buf.ptr(), len);
//             buf.into_box(slice.len()).assume_init()
//         }
//     }
// }

// #[cfg(not(no_global_oom_handling))]
// #[stable(feature = "box_from_cow", since = "1.45.0")]
// impl<T: Copy> From<Cow<'_, [T]>> for Box<[T]> {
//     /// Converts a `Cow<'_, [T]>` into a `Box<[T]>`
//     ///
//     /// When `cow` is the `Cow::Borrowed` variant, this
//     /// conversion allocates on the heap and copies the
//     /// underlying slice. Otherwise, it will try to reuse the owned
//     /// `Vec`'s allocation.
//     #[inline]
//     fn from(cow: Cow<'_, [T]>) -> Box<[T]> {
//         match cow {
//             Cow::Borrowed(slice) => Box::from(slice),
//             Cow::Owned(slice) => Box::from(slice),
//         }
//     }
// }

// #[cfg(not(no_global_oom_handling))]
// #[stable(feature = "box_from_slice", since = "1.17.0")]
// impl From<&str> for Box<str> {
//     /// Converts a `&str` into a `Box<str>`
//     ///
//     /// This conversion allocates on the heap
//     /// and performs a copy of `s`.
//     ///
//     /// # Examples
//     ///
//     /// ```rust
//     /// let boxed: Box<str> = Box::from("hello");
//     /// println!("{}", boxed);
//     /// ```
//     #[inline]
//     fn from(s: &str) -> Box<str> {
//         unsafe { from_boxed_utf8_unchecked(Box::from(s.as_bytes())) }
//     }
// }

// #[cfg(not(no_global_oom_handling))]
// #[stable(feature = "box_from_cow", since = "1.45.0")]
// impl From<Cow<'_, str>> for Box<str> {
//     /// Converts a `Cow<'_, str>` into a `Box<str>`
//     ///
//     /// When `cow` is the `Cow::Borrowed` variant, this
//     /// conversion allocates on the heap and copies the
//     /// underlying `str`. Otherwise, it will try to reuse the owned
//     /// `String`'s allocation.
//     ///
//     /// # Examples
//     ///
//     /// ```rust
//     /// use std::borrow::Cow;
//     ///
//     /// let unboxed = Cow::Borrowed("hello");
//     /// let boxed: Box<str> = Box::from(unboxed);
//     /// println!("{}", boxed);
//     /// ```
//     ///
//     /// ```rust
//     /// # use std::borrow::Cow;
//     /// let unboxed = Cow::Owned("hello".to_string());
//     /// let boxed: Box<str> = Box::from(unboxed);
//     /// println!("{}", boxed);
//     /// ```
//     #[inline]
//     fn from(cow: Cow<'_, str>) -> Box<str> {
//         match cow {
//             Cow::Borrowed(s) => Box::from(s),
//             Cow::Owned(s) => Box::from(s),
//         }
//     }
// }

// #[stable(feature = "boxed_str_conv", since = "1.19.0")]
// impl<A: Allocator> From<Box<str, A>> for Box<[u8], A> {
//     /// Converts a `Box<str>` into a `Box<[u8]>`
//     ///
//     /// This conversion does not allocate on the heap and happens in place.
//     ///
//     /// # Examples
//     /// ```rust
//     /// // create a Box<str> which will be used to create a Box<[u8]>
//     /// let boxed: Box<str> = Box::from("hello");
//     /// let boxed_str: Box<[u8]> = Box::from(boxed);
//     ///
//     /// // create a &[u8] which will be used to create a Box<[u8]>
//     /// let slice: &[u8] = &[104, 101, 108, 108, 111];
//     /// let boxed_slice = Box::from(slice);
//     ///
//     /// assert_eq!(boxed_slice, boxed_str);
//     /// ```
//     #[inline]
//     fn from(s: Box<str, A>) -> Self {
//         let (raw, alloc) = Box::into_raw_with_allocator(s);
//         unsafe { Box::from_raw_in(raw as *mut [u8], alloc) }
//     }
// }

// #[cfg(not(no_global_oom_handling))]
// #[stable(feature = "box_from_array", since = "1.45.0")]
// impl<T, const N: usize> From<[T; N]> for Box<[T]> {
//     /// Converts a `[T; N]` into a `Box<[T]>`
//     ///
//     /// This conversion moves the array to newly heap-allocated memory.
//     ///
//     /// # Examples
//     ///
//     /// ```rust
//     /// let boxed: Box<[u8]> = Box::from([4, 2]);
//     /// println!("{:?}", boxed);
//     /// ```
//     fn from(array: [T; N]) -> Box<[T]> {
//         box array
//     }
// }

// #[stable(feature = "boxed_slice_try_from", since = "1.43.0")]
// impl<T, const N: usize> TryFrom<Box<[T]>> for Box<[T; N]> {
//     type Error = Box<[T]>;

//     /// Attempts to convert a `Box<[T]>` into a `Box<[T; N]>`.
//     ///
//     /// The conversion occurs in-place and does not require a
//     /// new memory allocation.
//     ///
//     /// # Errors
//     ///
//     /// Returns the old `Box<[T]>` in the `Err` variant if
//     /// `boxed_slice.len()` does not equal `N`.
//     fn try_from(boxed_slice: Box<[T]>) -> Result<Self, Self::Error> {
//         if boxed_slice.len() == N {
//             Ok(unsafe { Box::from_raw(Box::into_raw(boxed_slice) as *mut [T; N]) })
//         } else {
//             Err(boxed_slice)
//         }
//     }
// }

impl Box<dyn Any> {
    #[inline]
    pub fn downcast<T: Any>(self) -> Result<Box<T>, Self> {
        // NOTE(wyj): is check is performed on *self, i.e., dyn Any
        if self.is::<T>() {
            unsafe { Ok(self.downcast_unchecked::<T>()) }
        } else {
            Err(self)
        }
    }

    #[inline]
    pub unsafe fn downcast_unchecked<T: Any>(self) -> Box<T> {
        debug_assert!(self.is::<T>());
        let raw: *mut dyn Any = Box::into_raw(self).0;
        Box::from_raw_query_remote(raw as *mut T)
    }
}

impl Box<dyn Any + Send> {
    #[inline]
    pub fn downcast<T: Any>(self) -> Result<Box<T>, Self> {
        if self.is::<T>() {
            unsafe { Ok(self.downcast_unchecked::<T>()) }
        } else {
            Err(self)
        }
    }

    #[inline]
    pub unsafe fn downcast_unchecked<T: Any>(self) -> Box<T> {
        debug_assert!(self.is::<T>());
        let raw: *mut (dyn Any + Send) = Box::into_raw(self).0;
        Box::from_raw_query_remote(raw as *mut T)
    }
}

impl Box<dyn Any + Send + Sync> {
    #[inline]
    pub fn downcast<T: Any>(self) -> Result<Box<T>, Self> {
        if self.is::<T>() {
            unsafe { Ok(self.downcast_unchecked::<T>()) }
        } else {
            Err(self)
        }
    }

    #[inline]
    pub unsafe fn downcast_unchecked<T: Any>(self) -> Box<T> {
        debug_assert!(self.is::<T>());
        let raw: *mut (dyn Any + Send + Sync) = Box::into_raw(self).0;
        Box::from_raw_query_remote(raw as *mut T)
    }
}

impl<T: fmt::Display + ?Sized> fmt::Display for Box<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<T: fmt::Debug + ?Sized> fmt::Debug for Box<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized> fmt::Pointer for Box<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ptr: *const T = &**self;
        fmt::Pointer::fmt(&ptr, f)
    }
}

impl<T: ?Sized> Deref for Box<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { self.0.as_ref() }
    }
}

impl<T: ?Sized> DerefMut for Box<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { self.0.as_mut() }
    }
}

impl<I: Iterator + ?Sized> Iterator for Box<I> {
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

impl<I: Iterator + ?Sized> BoxIter for Box<I> {
    type Item = I::Item;
    default fn last(self) -> Option<I::Item> {
        #[inline]
        fn some<T>(_: Option<T>, x: T) -> Option<T> {
            Some(x)
        }

        self.fold(None, some)
    }
}

impl<I: Iterator> BoxIter for Box<I> {
    fn last(self) -> Option<I::Item> {
        Box::into_inner(self).last()
    }
}

impl<I: DoubleEndedIterator + ?Sized> DoubleEndedIterator for Box<I> {
    fn next_back(&mut self) -> Option<I::Item> {
        (**self).next_back()
    }
    fn nth_back(&mut self, n: usize) -> Option<I::Item> {
        (**self).nth_back(n)
    }
}
impl<I: ExactSizeIterator + ?Sized> ExactSizeIterator for Box<I> {
    fn len(&self) -> usize {
        (**self).len()
    }
    fn is_empty(&self) -> bool {
        (**self).is_empty()
    }
}

impl<I: FusedIterator + ?Sized> FusedIterator for Box<I> {}

// impl<Args, F: FnOnce<Args> + ?Sized> FnOnce<Args> for Box<F> {
//     type Output = <F as FnOnce<Args>>::Output;

//     extern "rust-call" fn call_once(self, args: Args) -> Self::Output {
//         <F as FnOnce<Args>>::call_once(*self, args)
//     }
// }

// impl<Args, F: FnMut<Args> + ?Sized> FnMut<Args> for Box<F> {
//     extern "rust-call" fn call_mut(&mut self, args: Args) -> Self::Output {
//         <F as FnMut<Args>>::call_mut(self, args)
//     }
// }

// impl<Args, F: Fn<Args> + ?Sized> Fn<Args> for Box<F> {
//     extern "rust-call" fn call(&self, args: Args) -> Self::Output {
//         <F as Fn<Args>>::call(self, args)
//     }
// }

// impl<T: ?Sized + Unsize<U>, U: ?Sized> CoerceUnsized<Box<U>> for Box<T> {}

// impl<T: ?Sized + Unsize<U>, U: ?Sized> DispatchFromDyn<Box<U>> for Box<T> {}

// #[cfg(not(no_global_oom_handling))]
// #[stable(feature = "boxed_slice_from_iter", since = "1.32.0")]
// impl<I> FromIterator<I> for Box<[I]> {
//     fn from_iter<T: IntoIterator<Item = I>>(iter: T) -> Self {
//         iter.into_iter().collect::<Vec<_>>().into_boxed_slice()
//     }
// }

// #[cfg(not(no_global_oom_handling))]
// #[stable(feature = "box_slice_clone", since = "1.3.0")]
// impl<T: Clone, A: Allocator + Clone> Clone for Box<[T], A> {
//     fn clone(&self) -> Self {
//         let alloc = Box::allocator(self).clone();
//         self.to_vec_in(alloc).into_boxed_slice()
//     }

//     fn clone_from(&mut self, other: &Self) {
//         if self.len() == other.len() {
//             self.clone_from_slice(&other);
//         } else {
//             *self = other.clone();
//         }
//     }
// }

impl<T: ?Sized> borrow::Borrow<T> for Box<T> {
    fn borrow(&self) -> &T {
        &**self
    }
}

impl<T: ?Sized> borrow::BorrowMut<T> for Box<T> {
    fn borrow_mut(&mut self) -> &mut T {
        &mut **self
    }
}

impl<T: ?Sized> AsRef<T> for Box<T> {
    fn as_ref(&self) -> &T {
        &**self
    }
}

impl<T: ?Sized> AsMut<T> for Box<T> {
    fn as_mut(&mut self) -> &mut T {
        &mut **self
    }
}

impl<T: ?Sized> Unpin for Box<T> {}
