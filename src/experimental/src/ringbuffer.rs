use crossbeam::utils::{Backoff, CachePadded};
use std::alloc::{Allocator, Global};
use std::mem::{self, MaybeUninit};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct RingBuffer<T: Sized, A: Allocator = Global> {
    cap: usize,
    mask: CachePadded<usize>,
    head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
    data: *mut T,
    alloc: A,
}

unsafe impl<T, A: Allocator> Send for RingBuffer<T, A> {}
unsafe impl<T, A: Allocator> Sync for RingBuffer<T, A> {}

impl<T, A: Allocator> Drop for RingBuffer<T, A> {
    fn drop(&mut self) {
        // safety: Allocator is a ZST zero-sized type, no real drop over it
        let alloc = mem::replace(&mut self.alloc, unsafe {
            MaybeUninit::uninit().assume_init()
        });
        let _ = unsafe { Vec::from_raw_parts_in(self.data, 0, self.cap, alloc) };
    }
}

impl<T: Sized> RingBuffer<T, Global> {
    pub fn with_capacity(count: usize) -> Self {
        RingBuffer::with_capacity_in(count, Global)
    }
}

impl<T: Sized, A: Allocator> RingBuffer<T, A> {
    pub fn with_capacity_in(count: usize, alloc: A) -> Self {
        let capacity = count.next_power_of_two();
        assert!(capacity > 0);

        let v: Vec<T, A> = Vec::with_capacity_in(capacity, alloc);
        let (ptr, len, cap, alloc) = v.into_raw_parts_with_alloc();
        assert_eq!(len, 0);
        assert_eq!(cap, capacity);

        RingBuffer {
            cap: capacity,
            mask: CachePadded::new(capacity - 1),
            head: CachePadded::new(AtomicUsize::new(0)),
            tail: CachePadded::new(AtomicUsize::new(0)),
            data: ptr,
            alloc,
        }
    }

    pub fn push(&self, new_value: T) {
        let mut cur_head = self.head.load(Ordering::Relaxed);
        let mut cur_tail = self.tail.load(Ordering::Relaxed);
        if cur_tail >= cur_head + self.cap {
            let backoff = Backoff::new();
            while cur_tail >= cur_head + self.cap {
                backoff.snooze();
                cur_head = self.head.load(Ordering::Relaxed);
                cur_tail = self.tail.load(Ordering::Relaxed);
            }
        }
        unsafe {
            self.data
                .add(cur_tail & self.mask.into_inner())
                .write(new_value);
        }
        self.tail.store(cur_tail + 1, Ordering::Release);
    }

    pub fn wait_and_pop(&self) -> T {
        let mut x = self.try_pop();
        if x.is_none() {
            let backoff = Backoff::new();
            while x.is_none() {
                backoff.spin();
                x = self.try_pop();
            }
        }
        return x.unwrap();
    }

    pub fn try_push(&self, new_value: T) -> Result<(), T> {
        let cur_head = self.head.load(Ordering::Relaxed);
        let cur_tail = self.tail.load(Ordering::Relaxed);
        if cur_tail >= cur_head + self.cap {
            return Err(new_value);
        }
        unsafe {
            self.data
                .add(cur_tail & self.mask.into_inner())
                .write(new_value);
        }
        self.tail.store(cur_tail + 1, Ordering::Release);
        Ok(())
    }

    pub fn try_pop(&self) -> Option<T> {
        let cur_head = self.head.load(Ordering::Relaxed);
        let cur_tail = self.tail.load(Ordering::Acquire);
        if cur_tail == cur_head {
            return None;
        }
        let value = unsafe { self.data.add(cur_head & self.mask.into_inner()).read() };
        self.head.store(cur_head + 1, Ordering::Relaxed);
        Some(value)
    }
}
