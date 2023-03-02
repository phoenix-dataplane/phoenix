//! Re-exports of some types in IPC-channel crate.
//! It also provides an IpcSenderNotify class.
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Serialize;

use crate::shmobj::ShmObject;

pub use ipc_channel::ipc::TryRecvError;
pub(crate) use ipc_channel::ipc::{
    channel, IpcError as IpcRecvError, IpcOneShotServer as OneShotServer, IpcReceiver, IpcSender,
};
pub(crate) use ipc_channel::Error as IpcSendError;

pub struct IpcSenderNotify<T> {
    inner: IpcSender<T>,
    entries: ShmObject<AtomicUsize>,
}

impl<T: Serialize> IpcSenderNotify<T> {
    pub(crate) fn new(inner: IpcSender<T>, entries: ShmObject<AtomicUsize>) -> Self {
        IpcSenderNotify { inner, entries }
    }

    pub(crate) fn send(&self, data: T) -> Result<(), bincode::Error> {
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
