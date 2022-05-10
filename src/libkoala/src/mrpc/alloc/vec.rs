use std::convert::{AsMut, AsRef};
use std::ops::{Deref, DerefMut};

use ipc::shmalloc::SwitchAddressSpace;

use crate::salloc::heap::SharedHeapAllocator;

pub struct Vec<T> {
    inner: std::vec::Vec<T, SharedHeapAllocator>,
}

impl<T> Vec<T> {
    #[inline]
    pub const fn new() -> Self {
        Vec {
            inner: std::vec::Vec::new_in(SharedHeapAllocator),
        }
    }

    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Vec {
            inner: std::vec::Vec::with_capacity_in(capacity, SharedHeapAllocator),
        }
    }

    #[inline]
    pub unsafe fn from_raw_parts(ptr: *mut T, length: usize, capacity: usize) -> Self {
        Vec {
            inner: std::vec::Vec::from_raw_parts_in(ptr, length, capacity, SharedHeapAllocator),
        }
    }
}

impl<T> Deref for Vec<T> {
    type Target = std::vec::Vec<T, SharedHeapAllocator>;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for Vec<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T> AsRef<[T]> for Vec<T> {
    #[inline]
    fn as_ref(&self) -> &[T] {
        &self.inner
    }
}

impl<T> AsMut<[T]> for Vec<T> {
    #[inline]
    fn as_mut(&mut self) -> &mut [T] {
        &mut self.inner
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Vec<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T: Clone> Clone for Vec<T> {
    fn clone(&self) -> Self {
        Vec {
            inner: self.inner.clone(),
        }
    }
}

impl<T> IntoIterator for Vec<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T, SharedHeapAllocator>;

    #[inline]
    fn into_iter(self) -> std::vec::IntoIter<T, SharedHeapAllocator> {
        self.inner.into_iter()
    }
}

unsafe impl<T: SwitchAddressSpace> SwitchAddressSpace for Vec<T> {
    fn switch_address_space(&mut self) {

    }
}

unsafe impl<T> SwitchAddressSpace for Vec<T> {
    default fn switch_address_space(&mut self) {

    }
}