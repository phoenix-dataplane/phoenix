#![feature(unix_socket_ancillary_data)]
#![feature(peer_credentials_unix_socket)]
#![feature(slice_index_methods)]
pub use ipc_channel::ipc::{
    channel, IpcError, IpcOneShotServer as OneShotServer, IpcReceiver,
    IpcReceiverSet as ReceiverSet, IpcSelectionResult, IpcSender, IpcSharedMemory, TryRecvError,
};

pub use ipc_channel::Error;

pub use shmem_ipc::ringbuf::Error as ShmRingbufError;
pub use shmem_ipc::sharedring::{Receiver as ShmReceiver, Sender as ShmSender};
pub use shmem_ipc::Error as ShmIpcError;

pub mod control;
pub mod transport;

pub mod buf;

pub mod unix;

pub mod shm;
pub use shm::ShmObject;

use serde::Serialize;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct IpcSenderNotify<T> {
    inner: IpcSender<T>,
    entries: ShmObject<AtomicUsize>,
}

impl<T: Serialize> IpcSenderNotify<T> {
    pub fn new(inner: IpcSender<T>, entries: ShmObject<AtomicUsize>) -> Self {
        IpcSenderNotify { inner, entries }
    }

    pub fn send(&self, data: T) -> Result<(), bincode::Error> {
        self.inner.send(data)?;
        self.entries.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

impl<T> Clone for IpcSenderNotify<T>
where
    T: Serialize,
{
    fn clone(&self) -> IpcSenderNotify<T> {
        IpcSenderNotify {
            inner: self.inner.clone(),
            entries: self.entries.clone(),
        }
    }
}
