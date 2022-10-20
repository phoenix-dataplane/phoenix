//! Sequential Customer implementation. The two endpoints are must
//! be in the same thread (means no concurrency). The queue can be
//! extremely efficient.
//!
use std::cell::RefCell;
use std::collections::VecDeque;
use std::os::unix::io::RawFd;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::queue::Queue;
use crate::{Error, TryRecvError};

// TODO(cjr): make these configurable, see phoenix.toml
const DP_WQ_DEPTH: usize = 32;
const DP_CQ_DEPTH: usize = 32;

pub struct Shared<A, B, C, D> {
    fd_queue: VecDeque<Vec<RawFd>>,
    cmd_a: VecDeque<A>,
    cmd_b: VecDeque<B>,
    dp_c: Queue<C>,
    dp_d: Queue<D>,
}

impl<A, B, C, D> Shared<A, B, C, D> {
    pub fn new() -> Self {
        Self {
            fd_queue: VecDeque::new(),
            cmd_a: VecDeque::new(),
            cmd_b: VecDeque::new(),
            dp_c: Queue::new(DP_WQ_DEPTH),
            dp_d: Queue::new(DP_CQ_DEPTH),
        }
    }
}

/// # Safety
///
/// A Customer can be safely sent only when its corresponding Service is also moved
/// to the same thread at the same time. The engine runtime must guarantee that.
unsafe impl<A: Send, B: Send, C: Send, D: Send> Send for Customer<A, B, C, D> {}
unsafe impl<A: Send, B: Send, C: Send, D: Send> Send for Service<A, B, C, D> {}

/// # Safety
///
/// The runtime guarantees that the inner Shared<A, B, C, D> always has two onwers,
/// and the two onwers are scheduled as a whole on the same runtime.
///
/// `ref_thread_count == 1` is always true. So, it is safe to mark Sync.
unsafe impl<A: Sync, B: Sync, C: Sync, D: Sync> Sync for Customer<A, B, C, D> {}
unsafe impl<A: Sync, B: Sync, C: Sync, D: Sync> Sync for Service<A, B, C, D> {}

pub struct Customer<Command, Completion, WorkRequest, WorkCompletion> {
    shared: Rc<RefCell<Shared<Command, Completion, WorkRequest, WorkCompletion>>>,
}

impl<Command, Completion, WorkRequest, WorkCompletion>
    Customer<Command, Completion, WorkRequest, WorkCompletion>
where
    Command: for<'de> Deserialize<'de> + Serialize,
    Completion: for<'de> Deserialize<'de> + Serialize,
    WorkRequest: Copy + zerocopy::FromBytes,
    WorkCompletion: Copy + zerocopy::AsBytes,
{
    pub fn new(
        shared: &Rc<RefCell<Shared<Command, Completion, WorkRequest, WorkCompletion>>>,
    ) -> Self {
        Self {
            shared: Rc::clone(shared),
        }
    }

    #[inline]
    pub(crate) fn has_control_command(&self) -> bool {
        self.shared.borrow().cmd_a.len() > 0
    }

    #[inline]
    pub(crate) fn send_fd(&self, fds: &[RawFd]) -> Result<(), Error> {
        self.shared.borrow_mut().fd_queue.push_back(fds.to_vec());
        Ok(())
    }

    #[inline]
    pub(crate) fn try_recv_cmd(&self) -> Result<Command, TryRecvError> {
        self.shared
            .borrow_mut()
            .cmd_a
            .pop_front()
            .ok_or(TryRecvError::Empty)
    }

    #[inline]
    pub(crate) fn send_comp(&self, comp: Completion) -> Result<(), Error> {
        self.shared.borrow_mut().cmd_b.push_back(comp);
        Ok(())
    }

    #[inline]
    pub(crate) fn get_avail_wr_count(&mut self) -> Result<usize, Error> {
        Ok(self.shared.borrow().dp_c.len())
    }

    #[inline]
    pub(crate) fn get_avail_wc_slots(&mut self) -> Result<usize, Error> {
        Ok(self.shared.borrow().dp_d.avail())
    }

    #[inline]
    pub(crate) fn dequeue_wr_with<F: FnOnce(*const WorkRequest, usize) -> usize>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        let (ptr, count) = self.shared.borrow().dp_c.get_data_buf();
        let read_cnt = f(ptr, count);
        assert!(read_cnt <= count);
        self.shared.borrow_mut().dp_c.read_advance(read_cnt);
        Ok(())
    }

    /// This will possibly trigger the eventfd.
    #[inline]
    pub(crate) fn notify_wc_with<F: FnOnce(*mut WorkCompletion, usize) -> usize>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        self.enqueue_wc_with(f)
    }

    /// This will bypass the eventfd, thus much faster.
    #[inline]
    pub(crate) fn enqueue_wc_with<F: FnOnce(*mut WorkCompletion, usize) -> usize>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        let (ptr, count) = self.shared.borrow().dp_d.get_avail_buf();
        let write_cnt = f(ptr, count);
        assert!(write_cnt <= count);
        unsafe {
            self.shared.borrow_mut().dp_d.write_advance(write_cnt);
        }
        Ok(())
    }
}

pub struct Service<Command, Completion, WorkRequest, WorkCompletion> {
    shared: Rc<RefCell<Shared<Command, Completion, WorkRequest, WorkCompletion>>>,
}

impl<Command, Completion, WorkRequest, WorkCompletion>
    Service<Command, Completion, WorkRequest, WorkCompletion>
where
    Command: for<'de> Deserialize<'de> + Serialize,
    Completion: for<'de> Deserialize<'de> + Serialize,
    WorkRequest: Copy + zerocopy::AsBytes,
    WorkCompletion: Copy + zerocopy::FromBytes,
{
    pub fn new(
        shared: &Rc<RefCell<Shared<Command, Completion, WorkRequest, WorkCompletion>>>,
    ) -> Self {
        Self {
            shared: Rc::clone(shared),
        }
    }

    #[inline]
    pub(crate) fn recv_fd(&self) -> Result<Vec<RawFd>, Error> {
        panic!("please use the non-blocking version")
    }

    #[inline]
    pub(crate) fn try_recv_fd(&self) -> Result<Vec<RawFd>, Error> {
        self.shared
            .borrow_mut()
            .fd_queue
            .pop_front()
            .ok_or(Error::TryRecv(TryRecvError::Empty))
    }

    #[inline]
    pub(crate) fn send_cmd(&self, cmd: Command) -> Result<(), Error> {
        self.shared.borrow_mut().cmd_a.push_back(cmd);
        Ok(())
    }

    #[inline]
    pub(crate) fn recv_comp(&self) -> Result<Completion, Error> {
        panic!("please use the non-blocking version")
    }

    #[inline]
    pub(crate) fn try_recv_comp(&self) -> Result<Completion, Error> {
        self.shared
            .borrow_mut()
            .cmd_b
            .pop_front()
            .ok_or(Error::TryRecv(TryRecvError::Empty))
    }

    #[inline]
    pub(crate) fn enqueue_wr_with<F: FnOnce(*mut WorkRequest, usize) -> usize>(
        &self,
        f: F,
    ) -> Result<(), Error> {
        let (ptr, count) = self.shared.borrow().dp_c.get_avail_buf();
        let write_cnt = f(ptr, count);
        assert!(write_cnt <= count);
        unsafe {
            self.shared.borrow_mut().dp_c.write_advance(write_cnt);
        }
        Ok(())
    }

    #[inline]
    pub(crate) fn dequeue_wc_with<F: FnOnce(*const WorkCompletion, usize) -> usize>(
        &self,
        f: F,
    ) -> Result<(), Error> {
        let (ptr, count) = self.shared.borrow().dp_d.get_data_buf();
        let read_cnt = f(ptr, count);
        assert!(read_cnt <= count);
        self.shared.borrow_mut().dp_d.read_advance(read_cnt);
        Ok(())
    }
}
