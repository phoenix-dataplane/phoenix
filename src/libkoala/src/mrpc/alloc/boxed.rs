use std::convert::{AsRef, AsMut};
use std::ops::{Deref, DerefMut};
use std::pin::Pin;

use crate::mrpc::shared_heap::SharedHeapAllocator;

// TODO(wyj): impl SwitchAddressSpace for our Box wrapper?
pub struct Box<T: ?Sized> {
    inner: std::boxed::Box<T, SharedHeapAllocator>
}

impl<T> Box<T> {
    #[inline(always)]
    pub fn new(x: T) -> Self {
        Box { inner: std::boxed::Box::new_in(x, SharedHeapAllocator) }        
    }

    #[inline(always)]
    pub fn pin(x: T) -> Pin<Self> {
        Box::new(x).into()
    }

    #[inline]
    pub fn unbox(self) -> T {
        *self.inner
    }
}

impl<T: ?Sized> Box<T> {
    #[inline]
    pub unsafe fn from_raw(raw: *mut T) -> Self {
        Box { inner: unsafe { std::boxed::Box::from_raw_in(raw, SharedHeapAllocator) } }
    }

    #[inline]
    pub fn into_raw(b: Self) -> *mut T {
        std::boxed::Box::into_raw(b.inner)
    }

    pub fn into_pin(boxed: Self) -> Pin<Self>
    {
        // It's not possible to move or replace the insides of a `Pin<Box<T>>`
        // when `T: !Unpin`,  so it's safe to pin it directly without any
        // additional requirements.
        unsafe { Pin::new_unchecked(boxed) }
    }    
}

impl<T: ?Sized> From<Box<T>> for Pin<Box<T>> {
    fn from(boxed: Box<T>) -> Self {
        Box::into_pin(boxed)
    }
}

// NOTE(wyj): directly deref to T, as Box has no method that takes &self, &mut self
// into_pin also depends on Deref
impl<T: ?Sized> Deref for Box<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}

impl<T: ?Sized> DerefMut for Box<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.inner
    }
}


impl<T: ?Sized> AsRef<T> for Box<T> {
    fn as_ref(&self) -> &T {
        self.inner.as_ref()
    }
}

impl<T: ?Sized> AsMut<T> for Box<T> {
    fn as_mut(&mut self) -> &mut T {
        self.inner.as_mut()
    }
}


impl<T: Clone> Clone for Box<T> {
    fn clone(&self) -> Self {
        Box { inner: self.inner.clone() }
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Box<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}