use std::ops::{Deref, DerefMut};
use std::collections::VecDeque;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedVecDeque<T> {
    queue: VecDeque<T>,
    max_bound: usize,
}

impl<T> BoundedVecDeque<T> {
    pub fn new() -> Self {
        BoundedVecDeque {
            queue: VecDeque::new(),
            max_bound: 0,
        }
    }

    pub fn set_bound(&mut self, max_bound: usize) {
        self.max_bound = max_bound;
    }

    pub fn push_back_checked(&mut self, val: T) -> Result<(), T> {
        if self.queue.len() < self.max_bound {
            self.queue.push_back(val);
            Ok(())
        } else {
            Err(val)
        }
    }
}

impl<T> Default for BoundedVecDeque<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Deref for BoundedVecDeque<T> {
    type Target = VecDeque<T>;
    fn deref(&self) -> &Self::Target {
        &self.queue
    }
}

impl<T> DerefMut for BoundedVecDeque<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.queue
    }
}
