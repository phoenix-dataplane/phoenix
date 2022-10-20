#[repr(align(4096))]
pub struct PagePadded<T>(T);

unsafe impl<T: Send> Send for PagePadded<T> {}
unsafe impl<T: Sync> Sync for PagePadded<T> {}

impl<T> PagePadded<T> {
    pub const fn new(t: T) -> PagePadded<T> {
        PagePadded::<T>(t)
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

use std::ops::{Deref, DerefMut};
impl<T> Deref for PagePadded<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for PagePadded<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

use std::fmt;
impl<T: fmt::Debug> fmt::Debug for PagePadded<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PagePadded")
            .field("value", &self.0)
            .finish()
    }
}

impl<T> From<T> for PagePadded<T> {
    fn from(t: T) -> Self {
        PagePadded::new(t)
    }
}
