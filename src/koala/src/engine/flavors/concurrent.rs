//! Concurrent channel, encapsulated over crossbeam channel.

pub(crate) use crossbeam::channel::{Receiver, Sender, unbounded};

pub(crate) fn create_channel<T>() -> (Sender<T>, Receiver<T>) {
    unbounded()
}
