//! Channel implementations.

pub(crate) mod flavors;

pub type SendError<T> = crossbeam::channel::SendError<T>;
pub type TryRecvError = crossbeam::channel::TryRecvError;

/// The sending of a channel.
#[derive(Debug)]
pub struct Sender<T> {
    flavor: SenderFlavor<T>,
}

#[derive(Debug)]
pub(crate) enum SenderFlavor<T> {
    /// Crossbeam MPMC channel.
    Concurrent(flavors::concurrent::Sender<T>),
    /// Sequential single-threaded queue. Not concurrent safe. Must be used with special
    /// scheduling policy.
    Sequential(flavors::sequential::Sender<T>),
}

macro_rules! choose_sender_flavor {
    ($flavor:expr, $func:ident $(, $args:tt)*) => {
        match $flavor {
            SenderFlavor::Concurrent(c) => c.$func($($args)*),
            SenderFlavor::Sequential(c) => c.$func($($args)*),
        }
    };
}

impl<T> Sender<T> {
    #[inline]
    pub fn send(&mut self, t: T) -> Result<(), SendError<T>> {
        choose_sender_flavor!(&mut self.flavor, send, t)
    }
}

/// The sending of a channel.
#[derive(Debug)]
pub struct Receiver<T> {
    flavor: ReceiverFlavor<T>,
}

#[derive(Debug)]
pub(crate) enum ReceiverFlavor<T> {
    /// Crossbeam MPMC channel.
    Concurrent(flavors::concurrent::Receiver<T>),
    /// Sequential single-threaded queue. Not concurrent safe. Must be used with special
    /// scheduling policy.
    Sequential(flavors::sequential::Receiver<T>),
}

macro_rules! choose_receiver_flavor {
    ($flavor:expr, $func:ident $(, $args:tt)*) => {
        match $flavor {
            ReceiverFlavor::Concurrent(c) => c.$func($($args)*),
            ReceiverFlavor::Sequential(c) => c.$func($($args)*),
        }
    };
}

impl<T> Receiver<T> {
    #[inline]
    pub fn try_recv(&mut self) -> Result<T, TryRecvError> {
        choose_receiver_flavor!(&mut self.flavor, try_recv)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        choose_receiver_flavor!(&self.flavor, is_empty)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelFlavor {
    Concurrent,
    Sequential,
}

pub fn create_channel<T>(flavor: ChannelFlavor) -> (Sender<T>, Receiver<T>) {
    match flavor {
        ChannelFlavor::Concurrent => {
            let (sender, receiver) = flavors::concurrent::create_channel();
            (
                Sender {
                    flavor: SenderFlavor::Concurrent(sender),
                },
                Receiver {
                    flavor: ReceiverFlavor::Concurrent(receiver),
                },
            )
        }
        ChannelFlavor::Sequential => {
            let (sender, receiver) = flavors::sequential::create_channel();
            (
                Sender {
                    flavor: SenderFlavor::Sequential(sender),
                },
                Receiver {
                    flavor: ReceiverFlavor::Sequential(receiver),
                },
            )
        }
    }
}
