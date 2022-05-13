use thiserror::Error;

use crate::resource::Error as ResourceError;

pub mod engine;
pub mod state;
pub mod module;
pub mod marshal;
pub mod codegen;

#[derive(Debug, Error)]
pub(crate) enum Error {
    // Below are errors that return to the user.
    #[error("Failed to set transport type")]
    TransportType,
    #[error("Resource error: {0}")]
    Resource(#[from] ResourceError),

    // Below are errors that does not return to the user.
    #[error("ipc-channel TryRecvError")]
    IpcTryRecv,
    #[error("Customer error: {0}")]
    Customer(#[from] ipc::Error),
}

impl From<Error> for interface::Error {
    fn from(other: Error) -> Self {
        interface::Error::Generic(other.to_string())
    }
}

#[derive(Error, Debug)]
pub(crate) enum DatapathError {
    #[error("Shared memory queue error: {0}.")]
    ShmIpc(#[from] ipc::shmem_ipc::ShmIpcError),
    #[error("Shared memory queue ringbuf error: {0}.")]
    ShmRingbuf(#[from] ipc::shmem_ipc::ShmRingbufError),
    #[error("Internal queue send error")]
    InternalQueueSend,
}

impl From<ipc::Error> for DatapathError {
    fn from(other: ipc::Error) -> Self {
        match other {
            ipc::Error::ShmIpc(e) => DatapathError::ShmIpc(e),
            ipc::Error::ShmRingbuf(e) => DatapathError::ShmRingbuf(e),
            _ => panic!(),
        }
    }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for DatapathError {
    fn from(_other: tokio::sync::mpsc::error::SendError<T>) -> Self {
        DatapathError::InternalQueueSend
    }
}
