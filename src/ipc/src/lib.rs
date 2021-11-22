#![feature(unix_socket_ancillary_data)]
pub use ipc_channel::ipc::{
    channel, IpcError, IpcOneShotServer as OneShotServer, IpcReceiver,
    IpcReceiverSet as ReceiverSet, IpcSelectionResult, IpcSender,
    IpcSharedMemory, TryRecvError,
};

pub use ipc_channel::Error;

pub use shmem_ipc::ringbuf::Error as ShmRingbufError;
pub use shmem_ipc::sharedring::{Receiver as ShmReceiver, Sender as ShmSender};
pub use shmem_ipc::Error as ShmIpcError;

pub mod buf;
pub mod cmd;
pub mod dp;

pub mod unix;
pub use unix::{recv_fd, send_fd};

pub mod shm;
pub use shm::ShmObject;

use std::sync::atomic::{AtomicUsize, Ordering};
use serde::Serialize;

pub struct IpcSenderNotify<T> {
    inner: IpcSender<T>,
    entries: ShmObject<AtomicUsize>,
}

impl<T: Serialize> IpcSenderNotify<T> {
    pub fn new(inner: IpcSender<T>, entries: ShmObject<AtomicUsize>) -> Self {
        IpcSenderNotify {
            inner,
            entries,
        }
    }

    pub fn send(&self, data: T) -> Result<(), bincode::Error> {
        self.inner.send(data)?;
        self.entries.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }
}

impl<T> Clone for IpcSenderNotify<T> where T: Serialize {
    fn clone(&self) -> IpcSenderNotify<T> {
        IpcSenderNotify {
            inner: self.inner.clone(),
            entries: self.entries.clone(),
        }
    }
}

