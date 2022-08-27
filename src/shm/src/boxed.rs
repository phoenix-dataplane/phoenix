use std::any::Any;
use std::borrow;
use std::cmp::Ordering;
use std::convert::From;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::iter::{FusedIterator, Iterator};
use std::marker::Unpin;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::ptr;
use std::ptr::NonNull;

use std::alloc::handle_alloc_error;
use std::alloc::{AllocError, Layout};

use crate::alloc::{ShmAllocator, System};
use crate::ptr::{ShmNonNull, ShmPtr};

/// A pointer type for shared memory heap allocation.
pub struct Box<T: ?Sized, A: ShmAllocator = System>(ShmPtr<T>, A);

impl<T> Box<T> {
    #[inline]
    pub fn new(x: T) -> Self {
        Box::new_in(x, System)
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

    #[inline]
    pub fn new_uninit() -> Box<mem::MaybeUninit<T>> {
        Box::new_uninit_in(System)
    }

    #[inline]
    pub fn try_new_uninit() -> Result<Box<mem::MaybeUninit<T>>, AllocError> {
        Box::try_new_uninit_in(System)
    }

    #[inline]
    pub fn new_zeroed() -> Box<mem::MaybeUninit<T>> {
        let layout = Layout::new::<mem::MaybeUninit<T>>();
        // NOTE: Prefer match over unwrap_or_else since closure sometimes not inlineable.
        // That would make code size bigger.
        match Box::try_new_zeroed() {
            Ok(m) => m,
            Err(_) => handle_alloc_error(layout),
        }
    }

    #[inline]
    pub fn try_new_zeroed() -> Result<Box<mem::MaybeUninit<T>>, AllocError> {
        Box::try_new_zeroed_in(System)
    }

    #[inline(always)]
    pub fn pin(x: T) -> Pin<Self> {
        Self::into_pin(Self::new(x))
    }

    #[inline]
    pub fn unbox(self) -> T {
        Self::into_inner(self)
    }
}

impl<T, A: ShmAllocator> Box<mem::MaybeUninit<T>, A> {
    #[inline]
    pub unsafe fn assume_init(self) -> Box<T, A> {
        let ((raw_app, raw_backend), alloc) = Box::into_raw_with_allocator(self);
        Box::from_raw_in(raw_app.cast(), raw_backend.cast(), alloc)
    }

    #[inline]
    pub fn write(mut boxed: Self, value: T) -> Box<T, A> {
        unsafe {
            (*boxed).write(value);
            boxed.assume_init()
        }
    }
}

impl<T, A: ShmAllocator> Box<[mem::MaybeUninit<T>], A> {
    #[inline]
    pub unsafe fn assume_init(self) -> Box<[T], A> {
        let ((raw_app, raw_backend), alloc) = Box::into_raw_with_allocator(self);
        Box::from_raw_in(raw_app as *mut [T], raw_backend as *mut [T], alloc)
    }
}

impl<T: ?Sized> Box<T> {
    #[inline]
    pub(crate) unsafe fn from_raw(ptr_app: *mut T, ptr_backend: *mut T) -> Self {
        Self::from_raw_in(ptr_app, ptr_backend, System)
    }

    #[inline]
    pub(crate) fn into_raw(b: Self) -> (*mut T, *mut T) {
        let leaked = Box::into_shmptr(b);
        let (ptr_app, ptr_backend) = leaked.to_raw_parts();
        (ptr_app.as_ptr(), ptr_backend.as_ptr())
    }

    #[inline]
    pub(crate) fn into_shmptr(b: Self) -> ShmPtr<T> {
        Self::into_shmptr_with_allocator(b).0
    }
}

impl<T, A: ShmAllocator> Box<T, A> {
    #[inline]
    pub fn new_in(x: T, alloc: A) -> Self {
        let mut boxed = Self::new_uninit_in(alloc);
        unsafe {
            boxed.as_mut_ptr().write(x);
            boxed.assume_init()
        }
    }

    #[inline]
    pub fn as_ptr(b: &Self) -> *mut T {
        b.0.as_ptr_app()
    }

    #[inline]
    pub fn to_raw_parts(b: &Self) -> (NonNull<T>, NonNull<T>) {
        b.0.to_raw_parts()
    }

    #[inline]
    pub fn try_new_zeroed_in(alloc: A) -> Result<Box<mem::MaybeUninit<T>, A>, AllocError> {
        let layout = Layout::new::<mem::MaybeUninit<T>>();
        let ptr = System.allocate_zeroed(layout)?.cast();
        let (ptr_app, ptr_backend) = ptr.to_raw_parts();
        unsafe {
            Ok(Box::from_raw_in(
                ptr_app.as_ptr(),
                ptr_backend.as_ptr(),
                alloc,
            ))
        }
    }

    #[inline]
    pub fn new_uninit_in(alloc: A) -> Box<mem::MaybeUninit<T>, A> {
        let layout = Layout::new::<mem::MaybeUninit<T>>();
        // NOTE: Prefer match over unwrap_or_else since closure sometimes not inlineable.
        // That would make code size bigger.
        match Box::try_new_uninit_in(alloc) {
            Ok(m) => m,
            Err(_) => handle_alloc_error(layout),
        }
    }

    #[inline]
    pub fn try_new_uninit_in(alloc: A) -> Result<Box<mem::MaybeUninit<T>, A>, AllocError> {
        let layout = Layout::new::<mem::MaybeUninit<T>>();
        let ptr = alloc.allocate(layout)?.cast();
        let (ptr_app, ptr_backend) = ptr.to_raw_parts();
        unsafe {
            Ok(Box::from_raw_in(
                ptr_app.as_ptr(),
                ptr_backend.as_ptr(),
                alloc,
            ))
        }
    }

    #[inline]
    pub fn into_inner(boxed: Self) -> T
    where
        Self: Drop,
    {
        let mut dst = mem::MaybeUninit::uninit();
        let non_null: ShmNonNull<T> = boxed.0.into();
        let alloc = unsafe { ptr::read(&boxed.1) };
        let src = non_null.as_ptr_app();
        unsafe {
            // copy the content from shared heap to stack
            ptr::copy_nonoverlapping(src.as_const(), dst.as_mut_ptr(), 1);
            // drop the content on the shared heap
            let size = core::intrinsics::size_of_val(src.as_const());
            let align = core::intrinsics::min_align_of_val(src.as_const());
            ptr::drop_in_place(src);
            // let non_null: ptr::NonNull<T> = ptr::NonNull::new_unchecked(src);
            let layout = Layout::from_size_align_unchecked(size, align);
            alloc.deallocate(non_null.cast(), layout);
            dst.assume_init()
        }
    }

    #[inline(always)]
    pub fn pin_in(x: T, alloc: A) -> Pin<Self> {
        Self::into_pin(Self::new_in(x, alloc))
    }

    #[inline]
    pub fn into_boxed_slice(boxed: Self) -> Box<[T], A> {
        let ((raw_app, raw_backend), alloc) = Box::into_raw_with_allocator(boxed);
        unsafe { Box::from_raw_in(raw_app as *mut [T; 1], raw_backend as *mut [T; 1], alloc) }
    }
}

impl<T: ?Sized, A: ShmAllocator> Box<T, A> {
    #[inline]
    pub(crate) unsafe fn from_raw_in(ptr_app: *mut T, ptr_backend: *mut T, alloc: A) -> Box<T, A> {
        Box(ShmPtr::new_unchecked(ptr_app, ptr_backend), alloc)
    }

    #[inline]
    pub(crate) unsafe fn from_shmptr_in(shmptr: ShmPtr<T>, alloc: A) -> Self {
        Box(shmptr, alloc)
    }

    #[inline]
    pub(crate) fn into_shmptr_with_allocator(b: Self) -> (ShmPtr<T>, A) {
        let alloc = unsafe { ptr::read(&b.1) };
        let p = mem::ManuallyDrop::new(b);
        (p.0, alloc)
    }

    #[inline]
    pub(crate) fn into_raw_with_allocator(b: Self) -> ((*mut T, *mut T), A) {
        let (leaked, alloc) = Box::into_shmptr_with_allocator(b);
        let (ptr_app, ptr_backend) = leaked.to_raw_parts();
        ((ptr_app.as_ptr(), ptr_backend.as_ptr()), alloc)
    }

    #[inline]
    pub(crate) fn leak<'a>(b: Self) -> &'a mut T {
        unsafe { &mut *mem::ManuallyDrop::new(b).0.as_ptr_app() }
    }

    #[inline]
    pub fn into_pin(boxed: Self) -> Pin<Self> {
        // It's not possible to move or replace the insides of a `Pin<Box<T>>`
        // when `T: !Unpin`, so it's safe to pin it directly without any
        // additional requirements.
        unsafe { Pin::new_unchecked(boxed) }
    }
}

impl<T: ?Sized, A: ShmAllocator> Drop for Box<T, A> {
    fn drop(&mut self) {
        // SAFETY: users of Box (i.e., developers) must ensure that the Box is
        // only used for the sender heap. There is currently two places in the
        // code that we directly create a Box on the receiver heap.
        // For those cases, the box must not be dropped.
        unsafe {
            let size = core::intrinsics::size_of_val(self.0.as_ref_app());
            let align = core::intrinsics::min_align_of_val(self.0.as_ref_app());
            let ptr: ShmNonNull<T> = self.0.into();
            ptr::drop_in_place(ptr.as_ptr_app());
            let layout = Layout::from_size_align_unchecked(size, align);
            self.1.deallocate(ptr.cast(), layout);
        }
    }
}

impl<T: Default> Default for Box<T> {
    fn default() -> Self {
        Box::new(T::default())
    }
}

#[inline]
pub unsafe fn from_boxed_utf8_unchecked(v: Box<[u8]>) -> Box<str> {
    let (ptr_app, ptr_backend) = Box::into_raw(v);
    Box::from_raw(ptr_app as *mut str, ptr_backend as *mut str)
}

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

impl<T: Clone, A: ShmAllocator + Clone> Clone for Box<T, A> {
    #[inline]
    fn clone(&self) -> Box<T, A> {
        // Pre-allocate memory to allow writing the cloned value directly.
        let mut boxed = Self::new_uninit_in(self.1.clone());
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

impl<T: ?Sized + PartialEq, A: ShmAllocator> PartialEq for Box<T, A> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(&**self, &**other)
    }
    #[inline]
    fn ne(&self, other: &Self) -> bool {
        PartialEq::ne(&**self, &**other)
    }
}

impl<T: ?Sized + PartialOrd, A: ShmAllocator> PartialOrd for Box<T, A> {
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

impl<T: ?Sized + Ord, A: ShmAllocator> Ord for Box<T, A> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        Ord::cmp(&**self, &**other)
    }
}

impl<T: ?Sized + Eq, A: ShmAllocator> Eq for Box<T, A> {}

impl<T: ?Sized + Hash, A: ShmAllocator> Hash for Box<T, A> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (**self).hash(state);
    }
}

impl<T: ?Sized + Hasher, A: ShmAllocator> Hasher for Box<T, A> {
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

impl<T: ?Sized, A: ShmAllocator> From<Box<T, A>> for Pin<Box<T, A>> {
    fn from(boxed: Box<T, A>) -> Self {
        Box::into_pin(boxed)
    }
}

impl<A: ShmAllocator> Box<dyn Any, A> {
    #[inline]
    pub fn downcast<T: Any>(self) -> Result<Box<T, A>, Self> {
        // NOTE(wyj): is check is performed on *self, i.e., dyn Any
        if self.is::<T>() {
            unsafe { Ok(self.downcast_unchecked::<T>()) }
        } else {
            Err(self)
        }
    }

    #[inline]
    pub unsafe fn downcast_unchecked<T: Any>(self) -> Box<T, A> {
        debug_assert!(self.is::<T>());
        let ((raw_app, ptr_backend), alloc): ((*mut dyn Any, _), _) =
            Box::into_raw_with_allocator(self);
        Box::from_raw_in(raw_app as *mut T, ptr_backend as *mut T, alloc)
    }
}

impl<A: ShmAllocator> Box<dyn Any + Send, A> {
    #[inline]
    pub fn downcast<T: Any>(self) -> Result<Box<T, A>, Self> {
        if self.is::<T>() {
            unsafe { Ok(self.downcast_unchecked::<T>()) }
        } else {
            Err(self)
        }
    }

    #[inline]
    pub unsafe fn downcast_unchecked<T: Any>(self) -> Box<T, A> {
        debug_assert!(self.is::<T>());
        let ((raw_app, ptr_backend), alloc): ((*mut (dyn Any + Send), _), _) =
            Box::into_raw_with_allocator(self);
        Box::from_raw_in(raw_app as *mut T, ptr_backend as *mut T, alloc)
    }
}

impl<A: ShmAllocator> Box<dyn Any + Send + Sync, A> {
    #[inline]
    pub fn downcast<T: Any>(self) -> Result<Box<T, A>, Self> {
        if self.is::<T>() {
            unsafe { Ok(self.downcast_unchecked::<T>()) }
        } else {
            Err(self)
        }
    }

    #[inline]
    pub unsafe fn downcast_unchecked<T: Any>(self) -> Box<T, A> {
        debug_assert!(self.is::<T>());
        let ((raw_app, ptr_backend), alloc): ((*mut (dyn Any + Send + Sync), _), _) =
            Box::into_raw_with_allocator(self);
        Box::from_raw_in(raw_app as *mut T, ptr_backend as *mut T, alloc)
    }
}

impl<T: fmt::Display + ?Sized, A: ShmAllocator> fmt::Display for Box<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<T: fmt::Debug + ?Sized, A: ShmAllocator> fmt::Debug for Box<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized, A: ShmAllocator> fmt::Pointer for Box<T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ptr: *const T = &**self;
        fmt::Pointer::fmt(&ptr, f)
    }
}

impl<T: ?Sized, A: ShmAllocator> Deref for Box<T, A> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { self.0.as_ref_app() }
    }
}

impl<T: ?Sized, A: ShmAllocator> DerefMut for Box<T, A> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { self.0.as_mut_app() }
    }
}

impl<I: Iterator + ?Sized, A: ShmAllocator> Iterator for Box<I, A> {
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

impl<I: Iterator + ?Sized, A: ShmAllocator> BoxIter for Box<I, A> {
    type Item = I::Item;
    default fn last(self) -> Option<I::Item> {
        #[inline]
        fn some<T>(_: Option<T>, x: T) -> Option<T> {
            Some(x)
        }

        self.fold(None, some)
    }
}

impl<I: Iterator, A: ShmAllocator> BoxIter for Box<I, A> {
    fn last(self) -> Option<I::Item> {
        Box::into_inner(self).last()
    }
}

impl<I: DoubleEndedIterator + ?Sized, A: ShmAllocator> DoubleEndedIterator for Box<I, A> {
    fn next_back(&mut self) -> Option<I::Item> {
        (**self).next_back()
    }
    fn nth_back(&mut self, n: usize) -> Option<I::Item> {
        (**self).nth_back(n)
    }
}
impl<I: ExactSizeIterator + ?Sized, A: ShmAllocator> ExactSizeIterator for Box<I, A> {
    fn len(&self) -> usize {
        (**self).len()
    }
    fn is_empty(&self) -> bool {
        (**self).is_empty()
    }
}

impl<I: FusedIterator + ?Sized, A: ShmAllocator> FusedIterator for Box<I, A> {}

impl<T: ?Sized, A: ShmAllocator> borrow::Borrow<T> for Box<T, A> {
    fn borrow(&self) -> &T {
        &**self
    }
}

impl<T: ?Sized, A: ShmAllocator> borrow::BorrowMut<T> for Box<T, A> {
    fn borrow_mut(&mut self) -> &mut T {
        &mut **self
    }
}

impl<T: ?Sized, A: ShmAllocator> AsRef<T> for Box<T, A> {
    fn as_ref(&self) -> &T {
        &**self
    }
}

impl<T: ?Sized, A: ShmAllocator> AsMut<T> for Box<T, A> {
    fn as_mut(&mut self) -> &mut T {
        &mut **self
    }
}

impl<T: ?Sized, A: ShmAllocator> Unpin for Box<T, A> {}
