//! xatu OS service
use std::os::unix::io::RawFd;

use serde::{Deserialize, Serialize};

use crate::flavors;
use crate::Error;

// Re-exports
pub use crate::flavors::sequential::Service as SeqService;
pub use crate::flavors::shm::Service as ShmService;

pub(crate) const MAX_MSG_LEN: usize = 65536;

pub struct Service<Command, Completion, WorkRequest, WorkCompletion> {
    pub(crate) flavor: ServiceFlavor<Command, Completion, WorkRequest, WorkCompletion>,
}

pub(crate) enum ServiceFlavor<A, B, C, D> {
    /// Customer interface based on shared memory.
    // SharedMemory(flavors::shm::Service<A, B, C, D>),
    /// Customer interface within a single address space (i.e., single process, but can be multi-threaded).
    Concurrent(flavors::concurrent::Service<A, B, C, D>),
    /// Customer interface where the two endpoints are guaranteed to be in the same thread (no concurrency).
    Sequential(flavors::sequential::Service<A, B, C, D>),
}

macro_rules! choose_flavor {
    ($flavor:expr, $func:ident $(, $args:tt)*) => {
        match $flavor {
            // ServiceFlavor::SharedMemory(c) => c.$func($($args)*),
            ServiceFlavor::Concurrent(c) => c.$func($($args)*),
            ServiceFlavor::Sequential(c) => c.$func($($args)*),
        }
    };
}

impl<Command, Completion, WorkRequest, WorkCompletion>
    Service<Command, Completion, WorkRequest, WorkCompletion>
where
    Command: for<'de> Deserialize<'de> + Serialize,
    Completion: for<'de> Deserialize<'de> + Serialize,
    WorkRequest: Copy + zerocopy::AsBytes,
    WorkCompletion: Copy + zerocopy::FromBytes,
{
    #[inline]
    pub fn recv_fd(&self) -> Result<Vec<RawFd>, Error> {
        choose_flavor!(&self.flavor, recv_fd)
    }

    #[inline]
    pub fn try_recv_fd(&self) -> Result<Vec<RawFd>, Error> {
        choose_flavor!(&self.flavor, try_recv_fd)
    }

    #[inline]
    pub fn send_cmd(&self, cmd: Command) -> Result<(), Error> {
        choose_flavor!(&self.flavor, send_cmd, cmd)
    }

    #[inline]
    pub fn recv_comp(&self) -> Result<Completion, Error> {
        choose_flavor!(&self.flavor, recv_comp)
    }

    #[inline]
    pub fn try_recv_comp(&self) -> Result<Completion, Error> {
        choose_flavor!(&self.flavor, try_recv_comp)
    }

    #[inline]
    pub fn enqueue_wr_with<F: FnOnce(*mut WorkRequest, usize) -> usize>(
        &self,
        f: F,
    ) -> Result<(), Error> {
        choose_flavor!(&self.flavor, enqueue_wr_with, f)
    }

    #[inline]
    pub fn dequeue_wc_with<F: FnOnce(*const WorkCompletion, usize) -> usize>(
        &self,
        f: F,
    ) -> Result<(), Error> {
        choose_flavor!(&self.flavor, dequeue_wc_with, f)
    }
}
