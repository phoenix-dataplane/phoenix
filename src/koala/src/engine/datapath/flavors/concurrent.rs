//! Concurrent channel, encapsulated over crossbeam channel.

pub(crate) use crossbeam::channel::{unbounded, Receiver, Sender};

pub(crate) fn create_channel<T>() -> (Sender<T>, Receiver<T>) {
    unbounded()
}
