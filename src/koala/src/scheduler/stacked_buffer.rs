use std::mem;
use std::mem::MaybeUninit;

/// A zero-cost buffer allocated on the stack.
/// Safety: users should manually care the safety, including uninitialized memory and boundary.
#[allow(unused)]
pub(crate) struct StackedBuffer<T: Sized, const COUNT: usize> {
    buf: [MaybeUninit<T>; COUNT],
    len: usize,
}

impl<T, const COUNT: usize> StackedBuffer<T, COUNT> {
    pub(crate) fn new() -> Self {
        StackedBuffer {
            buf: unsafe { MaybeUninit::uninit().assume_init() },
            len: 0,
        }
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub(crate) fn append(&mut self, val: T) {
        self.buf[self.len].write(val);
        self.len += 1;
    }

    #[inline(always)]
    pub(crate) fn as_slice(&self) -> &[T] {
        unsafe { mem::transmute::<_, &[T]>(&self.buf[0..self.len]) }
    }

    #[inline(always)]
    pub(crate) unsafe fn get_unchecked(&self, idx: usize) -> &T {
        mem::transmute::<_, &T>(&self.buf[idx])
    }
}
