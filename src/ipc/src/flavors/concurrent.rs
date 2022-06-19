//! Concurrent Customer implementation. The two endpoints must
//! be in the same process (the same address space). In this case,
//! OS resources can be shared easily (e.g., special memory regions, files).
use std::cell::RefCell;
use std::os::unix::io::RawFd;
use std::sync::Arc;

use crossbeam::channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};

use crate::shmem_ipc::{RingReceiver, RingSender};
use crate::{Error, IpcRecvError, IpcSendError, RecvFdError, TryRecvError};

// TODO(cjr): make these configurable, see koala.toml
const DP_WQ_DEPTH: usize = 32;
const DP_CQ_DEPTH: usize = 32;

impl From<crossbeam::channel::TryRecvError> for TryRecvError {
    fn from(other: crossbeam::channel::TryRecvError) -> Self {
        use crossbeam::channel::TryRecvError as ITRE;
        match other {
            ITRE::Empty => TryRecvError::Empty,
            ITRE::Disconnected => TryRecvError::Disconnected,
        }
    }
}

impl<T> From<crossbeam::channel::SendError<T>> for IpcSendError {
    fn from(_other: crossbeam::channel::SendError<T>) -> Self {
        IpcSendError::Crossbeam
    }
}

impl From<crossbeam::channel::RecvError> for IpcRecvError {
    fn from(_other: crossbeam::channel::RecvError) -> Self {
        // A message could not be received because the channel is empty and disconnected.
        IpcRecvError::Disconnected
    }
}

impl From<crossbeam::channel::RecvError> for RecvFdError {
    fn from(_other: crossbeam::channel::RecvError) -> Self {
        // A message could not be received because the channel is empty and disconnected.
        RecvFdError::Disconnected
    }
}

pub(crate) fn create_channel<A, B, C, D>() -> (Service<A, B, C, D>, Customer<A, B, C, D>)
where
    A: for<'de> Deserialize<'de> + Serialize,
    B: for<'de> Deserialize<'de> + Serialize,
    C: Copy + zerocopy::FromBytes + zerocopy::AsBytes,
    D: Copy + zerocopy::FromBytes + zerocopy::AsBytes,
{
    use crossbeam::channel::unbounded;
    let (fd_tx, fd_rx) = unbounded();
    let (cmd_tx, cmd_rx) = unbounded();
    let (comp_tx, comp_rx) = unbounded();

    let wq_cap_bytes = shmem_ipc::ringbuf::channel_bufsize::<C>(DP_WQ_DEPTH);
    let cq_cap_bytes = shmem_ipc::ringbuf::channel_bufsize::<D>(DP_CQ_DEPTH);

    let mut dp_wq_buf = vec![0; wq_cap_bytes];
    let mut dp_cq_buf = vec![0; cq_cap_bytes];
    let (dp_wq_tx, dp_wq_rx) = shmem_ipc::ringbuf::channel(dp_wq_buf.as_mut_slice());
    let (dp_cq_tx, dp_cq_rx) = shmem_ipc::ringbuf::channel(dp_cq_buf.as_mut_slice());
    let dp_wq_buf = Arc::new(dp_wq_buf);
    let dp_cq_buf = Arc::new(dp_cq_buf);

    (
        Service {
            fd_rx,
            cmd_tx,
            cmd_rx: comp_rx,
            dp_wq: RefCell::new(dp_wq_tx),
            dp_cq: RefCell::new(dp_cq_rx),
            _dp_wq_buf: Arc::clone(&dp_wq_buf),
            _dp_cq_buf: Arc::clone(&dp_cq_buf),
        },
        Customer {
            fd_tx,
            cmd_rx,
            cmd_tx: comp_tx,
            dp_wq: dp_wq_rx,
            dp_cq: dp_cq_tx,
            _dp_wq_buf: Arc::clone(&dp_wq_buf),
            _dp_cq_buf: Arc::clone(&dp_cq_buf),
        },
    )
}

/// # Safety
///
/// This is safe, because dp_wq and dp_cq require mutable reference to access. It is impossible to
/// alias a mutable reference.
unsafe impl<A: Sync, B: Sync, C: Sync, D: Sync> Sync for Customer<A, B, C, D> {}
unsafe impl<A: Sync, B: Sync, C: Sync, D: Sync> Sync for Service<A, B, C, D> {}

pub struct Customer<Command, Completion, WorkRequest, WorkCompletion> {
    fd_tx: Sender<Vec<RawFd>>,
    cmd_rx: Receiver<Command>,
    cmd_tx: Sender<Completion>,
    dp_wq: RingReceiver<WorkRequest>,
    dp_cq: RingSender<WorkCompletion>,

    _dp_wq_buf: Arc<Vec<u8>>,
    _dp_cq_buf: Arc<Vec<u8>>,
}

impl<Command, Completion, WorkRequest, WorkCompletion>
    Customer<Command, Completion, WorkRequest, WorkCompletion>
where
    Command: for<'de> Deserialize<'de> + Serialize,
    Completion: for<'de> Deserialize<'de> + Serialize,
    WorkRequest: Copy + zerocopy::FromBytes,
    WorkCompletion: Copy + zerocopy::AsBytes,
{
    #[inline]
    pub(crate) fn has_control_command(&self) -> bool {
        !self.cmd_rx.is_empty()
    }

    #[inline]
    pub(crate) fn send_fd(&self, fds: &[RawFd]) -> Result<(), Error> {
        Ok(self
            .fd_tx
            .send(fds.to_owned())
            .map_err(|e| Error::SendFd(Box::new(e)))?)
    }

    #[inline]
    pub(crate) fn try_recv_cmd(&self) -> Result<Command, TryRecvError> {
        let req = self.cmd_rx.try_recv()?;
        Ok(req)
    }

    #[inline]
    pub(crate) fn send_comp(&self, comp: Completion) -> Result<(), Error> {
        Ok(self
            .cmd_tx
            .send(comp)
            .map_err(|e| Error::IpcSend(e.into()))?)
    }

    #[inline]
    pub(crate) fn get_avail_wr_count(&mut self) -> Result<usize, Error> {
        Ok(self.dp_wq.read_count()?)
    }

    #[inline]
    pub(crate) fn get_avail_wc_slots(&mut self) -> Result<usize, Error> {
        Ok(self.dp_cq.write_count()?)
    }

    #[inline]
    pub(crate) fn dequeue_wr_with<F: FnOnce(*const WorkRequest, usize) -> usize>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        self.dp_wq.recv(f)?;
        Ok(())
    }

    /// This will possibly trigger the eventfd.
    #[inline]
    pub(crate) fn notify_wc_with<F: FnOnce(*mut WorkCompletion, usize) -> usize>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        // No any special processing here. Just forward to enqueue_wc
        self.enqueue_wc_with(f)
    }

    /// This will bypass the eventfd, thus much faster.
    #[inline]
    pub(crate) fn enqueue_wc_with<F: FnOnce(*mut WorkCompletion, usize) -> usize>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        self.dp_cq.send(f)?;
        Ok(())
    }
}

pub struct Service<Command, Completion, WorkRequest, WorkCompletion> {
    fd_rx: Receiver<Vec<RawFd>>,
    cmd_tx: Sender<Command>,
    cmd_rx: Receiver<Completion>,
    dp_wq: RefCell<RingSender<WorkRequest>>,
    dp_cq: RefCell<RingReceiver<WorkCompletion>>,

    _dp_wq_buf: Arc<Vec<u8>>,
    _dp_cq_buf: Arc<Vec<u8>>,
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
    pub(crate) fn recv_fd(&self) -> Result<Vec<RawFd>, Error> {
        let fds = self.fd_rx.recv().map_err(|e| Error::RecvFd(e.into()))?;
        Ok(fds)
    }

    #[inline]
    pub(crate) fn try_recv_fd(&self) -> Result<Vec<RawFd>, Error> {
        let fds = self
            .fd_rx
            .try_recv()
            .map_err(|e| Error::TryRecvFd(e.into()))?;
        Ok(fds)
    }

    #[inline]
    pub(crate) fn send_cmd(&self, cmd: Command) -> Result<(), Error> {
        Ok(self
            .cmd_tx
            .send(cmd)
            .map_err(|e| Error::IpcSend(e.into()))?)
    }

    #[inline]
    pub(crate) fn recv_comp(&self) -> Result<Completion, Error> {
        Ok(self.cmd_rx.recv().map_err(|e| Error::IpcRecv(e.into()))?)
    }

    #[inline]
    pub(crate) fn try_recv_comp(&self) -> Result<Completion, Error> {
        Ok(self
            .cmd_rx
            .try_recv()
            .map_err(|e| Error::TryRecv(e.into()))?)
    }

    #[inline]
    pub(crate) fn enqueue_wr_with<F: FnOnce(*mut WorkRequest, usize) -> usize>(
        &self,
        f: F,
    ) -> Result<(), Error> {
        self.dp_wq.borrow_mut().send(f)?;
        Ok(())
    }

    #[inline]
    pub(crate) fn dequeue_wc_with<F: FnOnce(*const WorkCompletion, usize) -> usize>(
        &self,
        f: F,
    ) -> Result<(), Error> {
        self.dp_cq.borrow_mut().recv(f)?;
        Ok(())
    }
}
