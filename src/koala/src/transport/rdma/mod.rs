use std::io;
use thiserror::Error;

pub mod engine;
pub mod module;
pub mod state;

use super::resource::Error as ResourceError;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("rdmacm internal error: {0}")]
    RdmaCm(io::Error),
    // #[error("ibv internal error: {0}")]
    // Ibv(io::Error),
    #[error("getaddrinfo error: {0}")]
    GetAddrInfo(io::Error),
    #[error("Resource not found in table")]
    NotFound,
    #[error("Resource exists in table")]
    Exists,
    #[error("Fail to create MemoryRegion: {0}")]
    MemoryRegion(rdma::mr::Error),
    #[error("Failed to send file descriptors: {0}")]
    SendFd(ipc::unix::Error),
    #[error("Operation in progress")]
    InProgress,
    #[error("Mio error: {0}")]
    Mio(io::Error),
    #[error("No CM event")]
    NoCmEvent,
    #[error("Transport specific error: {0}")]
    Transport(i32),

    // Below are errors that does not return to the user.
    #[error("ipc-channel TryRecvError")]
    IpcTryRecv,
    #[error("IPC send error: {0}")]
    IpcSend(#[from] ipc::Error),
}

impl From<Error> for interface::Error {
    fn from(other: Error) -> Self {
        interface::Error::Generic(other.to_string())
    }
}

impl From<ResourceError> for Error {
    fn from(other: ResourceError) -> Self {
        match other {
            ResourceError::NotFound => Error::NotFound,
            ResourceError::Exists => Error::Exists,
        }
    }
}

#[derive(Error, Debug)]
pub(crate) enum DatapathError {
    #[error("Resource not found in table.")]
    NotFound,
    #[error("Shared memory queue error: {0}.")]
    ShmIpc(#[from] ipc::ShmIpcError),
    #[error("Shared memory queue ringbuf error: {0}.")]
    ShmRingbuf(#[from] ipc::ShmRingbufError),
    #[error("rdmacm internal error: {0}.")]
    RdmaCm(io::Error),
    #[error("ibv internal error: {0}.")]
    Ibv(io::Error),
}

impl From<ResourceError> for DatapathError {
    fn from(other: ResourceError) -> Self {
        match other {
            ResourceError::NotFound => DatapathError::NotFound,
            ResourceError::Exists => panic!(),
        }
    }
}

impl DatapathError {
    pub(crate) fn into_vendor_err(self) -> u32 {
        match self {
            Self::NotFound => 1024,
            Self::ShmIpc(_) => 1025,
            Self::ShmRingbuf(_) => 1026,
            Self::RdmaCm(e) => e.raw_os_error().unwrap() as u32,
            Self::Ibv(e) => e.raw_os_error().unwrap() as u32,
        }
    }
}
