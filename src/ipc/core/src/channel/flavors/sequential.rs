//! Single producer single consumer queue. Non-thread-safe.
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use super::super::{SendError, TryRecvError};

#[derive(Debug)]
pub(crate) struct Sender<T> {
    shared: Rc<Shared<T>>,
}

// Rc is not safe to send. However, we (the developers) make sure when sending the Channel
// to another thread, all of this references (Senders, Receivers) will be moving to the
// same thread.
unsafe impl<T: Send> Send for Sender<T> {}

#[derive(Debug)]
pub(crate) struct Receiver<T> {
    shared: Rc<Shared<T>>,
}

// Rc is not safe to send. However, we (the developers) make sure when sending the Channel
// to another thread, all of this references (Senders, Receivers) will be moving to the
// same thread.
unsafe impl<T: Send> Send for Receiver<T> {}

#[derive(Debug)]
struct Inner<T> {
    queue: VecDeque<T>,
}

#[derive(Debug)]
struct Shared<T> {
    inner: RefCell<Inner<T>>,
}

impl<T> Sender<T> {
    pub(crate) fn send(&mut self, t: T) -> Result<(), SendError<T>> {
        if Rc::strong_count(&self.shared) == 1 {
            // cannot use type alias as constructor.
            return Err(crossbeam::channel::SendError(t));
        }
        let mut inner = self.shared.inner.borrow_mut();
        inner.queue.push_back(t);
        drop(inner);
        Ok(())
    }
}

impl<T> Receiver<T> {
    pub(crate) fn try_recv(&mut self) -> Result<T, TryRecvError> {
        if Rc::strong_count(&self.shared) == 1 {
            return Err(TryRecvError::Disconnected);
        }
        let mut inner = self.shared.inner.borrow_mut();
        match inner.queue.pop_front() {
            Some(t) => Ok(t),
            None => Err(TryRecvError::Empty),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        let inner = self.shared.inner.borrow_mut();
        inner.queue.is_empty()
    }
}

pub(crate) fn create_channel<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Inner {
        queue: VecDeque::new(),
    };
    let shared = Shared {
        inner: RefCell::new(inner),
    };
    let shared = Rc::new(shared);
    (
        Sender {
            shared: shared.clone(),
        },
        Receiver { shared },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ping_pong() {
        let (mut tx, mut rx) = create_channel();
        assert_eq!(tx.send(42), Ok(()));
        assert_eq!(rx.try_recv(), Ok(42));
    }

    #[test]
    fn closed_tx() {
        let (tx, mut rx) = create_channel::<()>();
        drop(tx);
        assert_eq!(rx.try_recv(), Err(TryRecvError::Disconnected));
    }

    #[test]
    fn closed_rx() {
        let (mut tx, rx) = create_channel();
        drop(rx);
        assert_eq!(tx.send(42), Err(crossbeam::channel::SendError(42)));
    }
}
