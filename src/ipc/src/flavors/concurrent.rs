//! Concurrent Customer implementation. The two endpoints must
//! be in the same process (the same address space). In this case,
//! OS resources can be shared easily (e.g., special memory regions, files).
use std::os::unix::io::RawFd;
use std::cell::RefCell;

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

pub struct Customer<Command, Completion, WorkRequest, WorkCompletion> {
    fd_tx: Sender<Vec<RawFd>>,
    cmd_rx: Receiver<Command>,
    cmd_tx: Sender<Completion>,
    dp_wq: RingReceiver<WorkRequest>,
    dp_cq: RingSender<WorkCompletion>,
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
        let fds = self.fd_rx.try_recv().map_err(|e| Error::TryRecvFd(e.into()))?;
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
        Ok(self.cmd_rx.try_recv().map_err(|e| Error::TryRecv(e.into()))?)
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
