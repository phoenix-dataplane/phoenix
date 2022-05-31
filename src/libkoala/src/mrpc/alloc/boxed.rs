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
use std::ptr::Unique;

use std::alloc::handle_alloc_error;
use std::alloc::{AllocError, Layout};

use ipc::shmalloc::{ShmPtr, SwitchAddressSpace};

use crate::salloc::heap::SharedHeapAllocator;
use crate::salloc::owner::{AppOwned, BackendOwned, AllocOwner};

use super::shmview::CloneFromBackendOwned;

// The declaration of the `Box` struct must be kept in sync with the
// `alloc::alloc::box_free` function or ICEs will happen. See the comment
// on `box_free` for more details.
pub struct Box<T: ?Sized, O: AllocOwner = AppOwned>(ShmPtr<T>, O);

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
        let (ptr, addr_remote) = SharedHeapAllocator.allocate(layout)?;
        let shmptr = unsafe { ShmPtr::new_unchecked(ptr.cast().as_ptr(), addr_remote) };
        Ok(Box(shmptr, AppOwned))
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
        let (ptr, addr_remote) = SharedHeapAllocator.allocate_zeroed(layout)?;
        let shmptr = unsafe { ShmPtr::new_unchecked(ptr.cast().as_ptr(), addr_remote) };
        Ok(Box(shmptr, AppOwned))
    }

    #[inline(always)]
    pub fn pin(x: T) -> Pin<Self> {
        Self::into_pin(Self::new(x))
    }

    pub fn into_boxed_slice(boxed: Self) -> Box<[T]> {
        let (raw, addr_remote) = Box::into_raw(boxed);
        unsafe { Box::from_raw(raw as *mut [T; 1], addr_remote) }
    }

    #[inline]
    pub fn into_inner(boxed: Self) -> T
    where
        Self: Drop,
    {
        let mut dst = std::mem::MaybeUninit::uninit();
        let (src, addr_remote) = Self::into_raw(boxed);
        unsafe {
            // copy the content from shared heap to stack
            std::ptr::copy_nonoverlapping(src.as_const(), dst.as_mut_ptr(), 1);
            // drop the content on the shared heap
            let size = core::intrinsics::size_of_val(src.as_const());
            let align = core::intrinsics::min_align_of_val(src.as_const());
            std::ptr::drop_in_place(src);
            let non_null: std::ptr::NonNull<T> = std::ptr::NonNull::new_unchecked(src);
            let layout = Layout::from_size_align_unchecked(size, align);
            SharedHeapAllocator.deallocate(non_null.cast(), addr_remote, layout);
            dst.assume_init()
        }
    }

    #[inline]
    pub fn unbox(self) -> T {
        Self::into_inner(self)
    }
}


impl<T> Box<std::mem::MaybeUninit<T>> {
    #[inline]
    pub unsafe fn assume_init(self) -> Box<T> {
        let (raw, addr_remote) = Box::into_raw(self);
        Box::from_raw(raw as *mut T, addr_remote)
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
        let (raw, addr_remote) = Box::into_raw(self);
        Box::from_raw(raw as *mut [T], addr_remote)
    }
}

impl<T: ?Sized, O: AllocOwner> Box<T, O> {
    #[inline]
    pub(crate) fn as_ptr(b: &Self) -> (*mut T, usize) {
        b.0.as_ptr()
    }
}

impl<T: ?Sized> Box<T, BackendOwned> {
    #[inline]
    pub(crate) unsafe fn from_backend_raw(raw: *mut T, addr_remote: usize) -> Self {
        Box(ShmPtr::new_unchecked(raw, addr_remote), BackendOwned)
    }

    pub(crate) unsafe fn from_backend_shmptr(raw: ShmPtr<T>) -> Self {
        Box(raw, BackendOwned)
    }
}

impl<T: ?Sized> Box<T, AppOwned> {
    #[inline]
    pub(crate) unsafe fn from_raw(raw: *mut T, addr_remote: usize) -> Self {
        Box(ShmPtr::new_unchecked(raw, addr_remote), AppOwned)
    }

    pub(crate) unsafe fn from_shmptr(raw: ShmPtr<T>) -> Self {
        Box(raw, AppOwned)
    }

    #[inline]
    pub(crate) fn into_raw(b: Self) -> (*mut T, usize) {
        let (leaked, addr_remote) = Box::into_unique(b);
        (leaked.as_ptr(), addr_remote)
    }

    #[inline]
    pub(crate) fn into_unique(b: Self) -> (Unique<T>, usize) {
        // Box is recognized as a "unique pointer" by Stacked Borrows, but internally it is a
        // raw pointer for the type system. Turning it directly into a raw pointer would not be
        // recognized as "releasing" the unique pointer to permit aliased raw accesses,
        // so all raw pointer methods have to go through `Box::leak`. Turning *that* to a raw pointer
        // behaves correctly.
        let addr_remote = b.0.get_remote_addr();
        (Unique::from(Box::leak(b)), addr_remote)
    }

    #[inline]
    pub(crate) fn into_shmptr<'a>(b: Self) -> ShmPtr<T> {
        mem::ManuallyDrop::new(b).0
    }

    #[inline]
    pub(crate) fn leak<'a>(b: Self) -> &'a mut T {
        unsafe { &mut *mem::ManuallyDrop::new(b).0.as_ptr().0 }
    }

    pub fn into_pin(boxed: Self) -> Pin<Self> {
        // It's not possible to move or replace the insides of a `Pin<Box<T>>`
        // when `T: !Unpin`,  so it's safe to pin it directly without any
        // additional requirements.
        unsafe { Pin::new_unchecked(boxed) }
    }
}


trait SpecializedDrop { 
    fn drop( &mut self ); 
}


impl<T: ?Sized> SpecializedDrop for Box<T, AppOwned> {
    fn drop(&mut self) {
        unsafe {
            let size = core::intrinsics::size_of_val(self.0.as_ref());
            let align = core::intrinsics::min_align_of_val(self.0.as_ref());
            let (ptr, addr_remote) = self.0.as_ptr();
            std::ptr::drop_in_place(ptr);
            let non_null: std::ptr::NonNull<T> = self.0.into();
            let layout = Layout::from_size_align_unchecked(size, align);
            SharedHeapAllocator.deallocate(non_null.cast(), addr_remote, layout);
        }
    }
}

impl<T: ?Sized> SpecializedDrop for Box<T, BackendOwned> {
    fn drop(&mut self) {
        // TODO(wyj)
        return;
    }
}

impl<T: ?Sized, O: AllocOwner> SpecializedDrop for Box<T, O> {
    default fn drop(&mut self) {
        unreachable!("SpecializedDrop should be specialized for AppOwned and BackendOwned");
    }
}

impl<T: ?Sized, O: AllocOwner> Drop for Box<T, O> {
    fn drop(&mut self) {
        (self as &mut dyn SpecializedDrop).drop();
    }
}

unsafe impl<T: ?Sized + SwitchAddressSpace> SwitchAddressSpace for Box<T> {
    fn switch_address_space(&mut self) {
        unsafe { self.0.as_mut().switch_address_space() };
        self.0.switch_address_space();
    }
}

impl<T: Default> Default for Box<T> {
    fn default() -> Self {
        Box::new(T::default())
    }
}

#[inline]
pub(crate) unsafe fn from_boxed_utf8_unchecked(v: Box<[u8], AppOwned>) -> Box<str, AppOwned> {
    let (ptr, addr_remote) = Box::into_raw(v);
    Box::from_raw(ptr as *mut str, addr_remote)
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

impl<T: Clone> Clone for Box<T> {
    #[inline]
    fn clone(&self) -> Box<T> {
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

impl<T: Clone> CloneFromBackendOwned for Box<T, AppOwned> {
    type BackendOwned = Box<T, BackendOwned>;

    fn clone_from_backend_owned(backend_owned: &Self::BackendOwned) -> Self {
        let mut boxed = Self::new_uninit();
        unsafe {
            (**backend_owned).write_clone_into_raw(boxed.as_mut_ptr());
            boxed.assume_init()
        }        
    }
}

impl<T: ?Sized + PartialEq, O: AllocOwner> PartialEq for Box<T, O> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(&**self, &**other)
    }
    #[inline]
    fn ne(&self, other: &Self) -> bool {
        PartialEq::ne(&**self, &**other)
    }
}

impl<T: ?Sized + PartialOrd, O: AllocOwner> PartialOrd for Box<T, O> {
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

impl<T: ?Sized + Ord, O: AllocOwner> Ord for Box<T, O> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        Ord::cmp(&**self, &**other)
    }
}

impl<T: ?Sized + Eq, O: AllocOwner> Eq for Box<T, O> {}

impl<T: ?Sized + Hash, O: AllocOwner> Hash for Box<T, O> {
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
        let (raw, addr_remote): (*mut dyn Any, _) = Box::into_raw(self);
        Box::from_raw(raw as *mut T, addr_remote)
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
        let (raw, addr_remote): (*mut (dyn Any + Send), _) = Box::into_raw(self);
        Box::from_raw(raw as *mut T, addr_remote)
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
        let (raw, addr_remote): (*mut (dyn Any + Send + Sync), _) = Box::into_raw(self);
        Box::from_raw(raw as *mut T, addr_remote)
    }
}

impl<T: fmt::Display + ?Sized, O: AllocOwner> fmt::Display for Box<T, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&**self, f)
    }
}

impl<T: fmt::Debug + ?Sized, O: AllocOwner> fmt::Debug for Box<T, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: ?Sized, O: AllocOwner> fmt::Pointer for Box<T, O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ptr: *const T = &**self;
        fmt::Pointer::fmt(&ptr, f)
    }
}

impl<T: ?Sized, O: AllocOwner> Deref for Box<T, O> {
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

impl<T: ?Sized, O: AllocOwner> borrow::Borrow<T> for Box<T, O> {
    fn borrow(&self) -> &T {
        &**self
    }
}

impl<T: ?Sized> borrow::BorrowMut<T> for Box<T> {
    fn borrow_mut(&mut self) -> &mut T {
        &mut **self
    }
}

impl<T: ?Sized, O: AllocOwner> AsRef<T> for Box<T, O> {
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