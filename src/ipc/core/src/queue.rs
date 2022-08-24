use std::mem;

const MAXIMUM_ZST_CAPACITY: usize = 1 << (usize::BITS - 1); // Largest possible power of two

pub struct Queue<T> {
    // tail and head are pointers into the buffer. Tail always points
    // to the first element that could be read, Head always points
    // to where data should be written.
    // If tail == head the buffer is empty. The length of the ringbuffer
    // is defined as the distance between the two.
    tail: usize,
    head: usize,
    buf: Vec<T>,
}

#[inline]
fn wrap_index(index: usize, size: usize) -> usize {
    // size is always a power of 2
    debug_assert!(size.is_power_of_two());
    index & (size - 1)
}

impl<T> Queue<T> {
    pub fn new(size: usize) -> Self {
        let mut buf = Vec::with_capacity(size);
        unsafe {
            buf.set_len(size);
        }
        Self {
            tail: 0,
            head: 0,
            buf,
        }
    }

    #[inline]
    unsafe fn ptr_offset(&self, offset: usize) -> *mut T {
        self.buf.as_ptr().offset(offset as isize) as _
    }

    #[inline]
    fn _wrap_index(&self, idx: usize) -> usize {
        wrap_index(idx, self.cap())
    }

    #[inline]
    fn wrap_add(&self, idx: usize, addend: usize) -> usize {
        wrap_index(idx.wrapping_add(addend), self.cap())
    }

    #[inline]
    fn _wrap_sub(&self, idx: usize, subtrahend: usize) -> usize {
        wrap_index(idx.wrapping_sub(subtrahend), self.cap())
    }

    #[inline]
    fn cap(&self) -> usize {
        if mem::size_of::<T>() == 0 {
            // For zero sized types, we are always at maximum capacity
            MAXIMUM_ZST_CAPACITY
        } else {
            self.buf.capacity()
        }
    }

    pub fn len(&self) -> usize {
        (self.head.wrapping_sub(self.tail)) & (self.cap() - 1)
    }

    pub fn avail(&self) -> usize {
        self.cap() - self.len() - 1
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.tail == self.head
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.avail() == 0
    }

    pub(crate) fn get_data_buf(&self) -> (*mut T, usize) {
        (
            // safe: self.tail is very small...
            unsafe { self.ptr_offset(self.tail) },
            if self.tail <= self.head {
                self.head - self.tail
            } else {
                self.cap() - self.tail
            },
        )
    }

    pub(crate) fn get_avail_buf(&self) -> (*mut T, usize) {
        let avail = self.avail();
        (
            unsafe { self.ptr_offset(self.head) },
            if avail == 0 {
                0
            } else if self.tail <= self.head {
                self.cap() - self.head + 1
            } else {
                avail
            },
        )
    }

    pub(crate) fn read_advance(&mut self, count: usize) -> usize {
        let count = count.min(self.len());
        self.tail = self.wrap_add(self.tail, count);
        count
    }

    /// # Safety:
    ///
    /// The user of this function must ensure that the appended values
    /// are valid data.
    pub(crate) unsafe fn write_advance(&mut self, count: usize) -> usize {
        let count = count.min(self.avail());
        self.head = self.wrap_add(self.head, count);
        count
    }
}
