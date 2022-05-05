//! xatu OS customer
use std::os::unix::io::RawFd;

use serde::{Deserialize, Serialize};

use crate::flavors;
use crate::{Error, TryRecvError};

// Re-exports
pub use crate::flavors::sequential::Customer as SeqCustomer;
pub use crate::flavors::shm::Customer as ShmCustomer;

/// COMMENT(cjr): There is no way to implement Customer as a trait. Because we need to
/// ask the user to pass in an F: FnOnce, thereby making the the trait not object-safe.
/// An object-safe trait means it can be used as an trait object.
/// https://doc.rust-lang.org/book/ch17-02-trait-objects.html#object-safety-is-required-for-trait-objects
///
/// As a result, like what has been done in crossbeam, where the channels have different flavors,
/// we use an enum to differentite different Customer implementations at runtime.
/// The runtime overhead is just a single branch instruction, whose prediction hit rate is 100%.
pub struct Customer<Command, Completion, WorkRequest, WorkCompletion> {
    pub(crate) flavor: CustomerFlavor<Command, Completion, WorkRequest, WorkCompletion>,
}

pub(crate) enum CustomerFlavor<A, B, C, D> {
    /// Customer interface based on shared memory.
    SharedMemory(flavors::shm::Customer<A, B, C, D>),
    /// Customer interface within a single address space (i.e., single process, but can be multi-threaded).
    Concurrent(flavors::concurrent::Customer<A, B, C, D>),
    /// Customer interface where the two endpoints are guaranteed to be in the same thread (no concurrency).
    Sequential(flavors::sequential::Customer<A, B, C, D>),
}

macro_rules! choose_flavor {
    ($flavor:expr, $func:ident $(, $args:tt)*) => {
        match $flavor {
            CustomerFlavor::SharedMemory(c) => c.$func($($args)*),
            CustomerFlavor::Concurrent(c) => c.$func($($args)*),
            CustomerFlavor::Sequential(c) => c.$func($($args)*),
        }
    };
}

impl<Command, Completion, WorkRequest, WorkCompletion>
    Customer<Command, Completion, WorkRequest, WorkCompletion>
where
    Command: for<'de> Deserialize<'de> + Serialize,
    Completion: for<'de> Deserialize<'de> + Serialize,
    WorkRequest: Copy + zerocopy::FromBytes,
    WorkCompletion: Copy + zerocopy::AsBytes,
{
    pub fn from_shm(
        shm_customer: ShmCustomer<Command, Completion, WorkRequest, WorkCompletion>,
    ) -> Self {
        Self {
            flavor: CustomerFlavor::SharedMemory(shm_customer),
        }
    }

    pub fn from_seq(
        seq_customer: SeqCustomer<Command, Completion, WorkRequest, WorkCompletion>,
    ) -> Self {
        Self {
            flavor: CustomerFlavor::Sequential(seq_customer),
        }
    }

    #[inline]
    pub fn has_control_command(&self) -> bool {
        match &self.flavor {
            CustomerFlavor::SharedMemory(c) => c.has_control_command(),
            CustomerFlavor::Concurrent(c) => c.has_control_command(),
            CustomerFlavor::Sequential(c) => c.has_control_command(),
        }
    }

    #[inline]
    pub fn send_fd(&self, fds: &[RawFd]) -> Result<(), Error> {
        choose_flavor!(&self.flavor, send_fd, fds)
    }

    #[inline]
    pub fn try_recv_cmd(&self) -> Result<Command, TryRecvError> {
        choose_flavor!(&self.flavor, try_recv_cmd)
    }

    #[inline]
    pub fn send_comp(&self, comp: Completion) -> Result<(), Error> {
        choose_flavor!(&self.flavor, send_comp, comp)
    }

    #[inline]
    pub fn get_avail_wr_count(&mut self) -> Result<usize, Error> {
        choose_flavor!(&mut self.flavor, get_avail_wr_count)
    }

    #[inline]
    pub fn get_avail_wc_slots(&mut self) -> Result<usize, Error> {
        choose_flavor!(&mut self.flavor, get_avail_wc_slots)
    }

    #[inline]
    pub fn dequeue_wr_with<F: FnOnce(*const WorkRequest, usize) -> usize>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        choose_flavor!(&mut self.flavor, dequeue_wr_with, f)
    }

    /// This will possibly trigger the eventfd.
    #[inline]
    pub fn notify_wc_with<F: FnOnce(*mut WorkCompletion, usize) -> usize>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        choose_flavor!(&mut self.flavor, notify_wc_with, f)
    }

    /// This will bypass the eventfd, thus much faster.
    #[inline]
    pub fn enqueue_wc_with<F: FnOnce(*mut WorkCompletion, usize) -> usize>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        choose_flavor!(&mut self.flavor, enqueue_wc_with, f)
    }
}
