#![feature(strict_provenance)]
#![feature(peer_credentials_unix_socket)]

use std::io;

use thiserror::Error;

pub use koala::module::KoalaModule;
pub use koala::plugin::InitFnResult;
use koala::resource::Error as ResourceError;

pub(crate) mod cm;
pub mod config;
pub(crate) mod engine;
pub mod module;
pub mod ops;
pub mod state;

#[derive(Debug, Error)]
pub enum ApiError {
    // Below are errors that return to the user.
    #[error("rdmacm internal error: {0}")]
    RdmaCm(io::Error),
    #[error("ibv internal error: {0}")]
    Ibv(io::Error),
    #[error("getaddrinfo error: {0}")]
    GetAddrInfo(io::Error),
    #[error("Resource not found in table")]
    NotFound,
    #[error("Resource exists in table")]
    Exists,
    #[error("Fail to create MemoryRegion: {0}")]
    MemoryRegion(rdma::mr::Error),
    #[error("Failed to send file descriptors: {0}")]
    SendFd(ipc::Error),
    #[error("Mio error: {0}")]
    Mio(io::Error),
    #[error("No CM event")]
    NoCmEvent,
    #[error("Transport specific error: {0}")]
    Transport(i32),
}

/// Control path error.
#[derive(Debug, Error)]
pub enum Error {
    #[error("Error in API: {0}")]
    Api(#[from] ApiError),

    // Below are errors that does not return to the user.
    #[error("ipc-channel TryRecvError")]
    IpcTryRecv,
    // #[error("IPC send error: {0}")]
    // IpcSend(#[from] ipc::Error),
    #[error("Customer error: {0}")]
    Customer(#[from] ipc::Error),
    #[error("Flushing datapath error: {0}")]
    FlushDp(#[from] DatapathError),
}

impl From<Error> for interface::Error {
    fn from(other: Error) -> Self {
        interface::Error::Generic(other.to_string())
    }
}

impl From<ResourceError> for ApiError {
    fn from(other: ResourceError) -> Self {
        match other {
            ResourceError::NotFound => ApiError::NotFound,
            ResourceError::Exists => ApiError::Exists,
        }
    }
}

#[derive(Error, Debug)]
pub enum DatapathError {
    #[error("Resource not found in table.")]
    NotFound,
    #[error("Shared memory queue error: {0}.")]
    ShmIpc(#[from] ipc::shmem_ipc::ShmIpcError),
    #[error("Shared memory queue ringbuf error: {0}.")]
    ShmRingbuf(#[from] ipc::shmem_ipc::ShmRingbufError),
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

impl From<ipc::Error> for DatapathError {
    fn from(other: ipc::Error) -> Self {
        match other {
            ipc::Error::ShmIpc(e) => DatapathError::ShmIpc(e),
            ipc::Error::ShmRingbuf(e) => DatapathError::ShmRingbuf(e),
            _ => panic!(),
        }
    }
}

impl DatapathError {
    pub fn into_vendor_err(self) -> u32 {
        match self {
            Self::NotFound => 1024,
            Self::ShmIpc(_) => 1025,
            Self::ShmRingbuf(_) => 1026,
            Self::RdmaCm(e) => e.raw_os_error().unwrap() as u32,
            Self::Ibv(e) => e.raw_os_error().unwrap() as u32,
        }
    }
}
