#![feature(strict_provenance)]
#![feature(peer_credentials_unix_socket)]

use std::io;

use thiserror::Error;

use phoenix_common::resource::Error as ResourceError;
pub use phoenix_common::{InitFnResult, PhoenixModule};

pub(crate) mod cm;
pub mod config;
pub(crate) mod engine;
pub mod module;

#[allow(clippy::too_many_arguments)]
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
    #[error("ResourceSlab is full")]
    SlabFull,
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

impl From<Error> for phoenix_api::Error {
    fn from(other: Error) -> Self {
        phoenix_api::Error::Generic(other.to_string())
    }
}

impl From<ResourceError> for ApiError {
    fn from(other: ResourceError) -> Self {
        match other {
            ResourceError::NotFound => ApiError::NotFound,
            ResourceError::Exists => ApiError::Exists,
            ResourceError::SlabFull => ApiError::SlabFull,
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
            ResourceError::SlabFull => panic!(),
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

use crate::config::RdmaTransportConfig;
use crate::module::RdmaTransportModule;
#[no_mangle]
pub fn init_module(config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixModule>> {
    let config = RdmaTransportConfig::new(config_string)?;
    let module = RdmaTransportModule::new(config);
    Ok(Box::new(module))
}
