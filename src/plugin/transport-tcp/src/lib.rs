#![feature(strict_provenance)]
#![feature(peer_credentials_unix_socket)]

use std::io;
use thiserror::Error;

pub use phoenix::module::PhoenixModule;
pub use phoenix::plugin::InitFnResult;
use phoenix::resource::Error as ResourceError;

pub mod config;
pub mod engine;
pub mod module;
pub mod ops;
pub(crate) mod state;
// pub(crate) mod mr;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("Socket internal error: {0}")]
    Socket(#[from] io::Error),
    #[error("Resource not found in table")]
    NotFound,
    // #[error("Fail to create MemoryRegion: {0}")]
    // MemoryRegion(mr::Error),
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("Error in API: {0}")]
    Api(#[from] ApiError),

    #[error("ipc-channel TryRecvError")]
    IpcTryRecv,
    #[error("Customer error: {0}")]
    Customer(#[from] ipc::Error),
    #[error("Flushing datapath error: {0}")]
    FlushDp(#[from] TransportError),
}

impl From<Error> for uapi::Error {
    fn from(other: Error) -> Self {
        uapi::Error::Generic(other.to_string())
    }
}

impl From<ResourceError> for ApiError {
    fn from(other: ResourceError) -> Self {
        match other {
            ResourceError::NotFound => ApiError::NotFound,
            ResourceError::Exists => panic!(),
            ResourceError::SlabFull => todo!(),
        }
    }
}

#[derive(Error, Debug)]
pub enum TransportError {
    #[error("Resource not found in table.")]
    NotFound,
    #[error("Shared memory queue error: {0}.")]
    ShmIpc(#[from] ipc::shmem_ipc::ShmIpcError),
    #[error("Shared memory queue ringbuf error: {0}.")]
    ShmRingbuf(#[from] ipc::shmem_ipc::ShmRingbufError),
    #[error("Socket internal error: {0}.")]
    Socket(#[from] io::Error),
    #[error("Disconnected")]
    Disconnected,
    #[error("General transport error: {0}")]
    General(String),
}

impl From<ResourceError> for TransportError {
    fn from(other: ResourceError) -> Self {
        match other {
            ResourceError::NotFound => TransportError::NotFound,
            ResourceError::Exists => panic!(),
            ResourceError::SlabFull => panic!(),
        }
    }
}

impl From<ipc::Error> for TransportError {
    fn from(other: ipc::Error) -> Self {
        match other {
            ipc::Error::ShmIpc(e) => TransportError::ShmIpc(e),
            ipc::Error::ShmRingbuf(e) => TransportError::ShmRingbuf(e),
            _ => panic!(),
        }
    }
}

impl TransportError {
    pub(crate) fn as_vendor_err(&self) -> u32 {
        match self {
            Self::NotFound => 1024,
            Self::ShmIpc(_) => 1025,
            Self::ShmRingbuf(_) => 1026,
            Self::Disconnected => 1027,
            Self::General(_) => 2048,
            Self::Socket(e) => e.raw_os_error().unwrap() as u32,
        }
    }
}

use crate::config::TcpTransportConfig;
use crate::module::TcpTransportModule;
#[no_mangle]
pub fn init_module(config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixModule>> {
    let config = TcpTransportConfig::new(config_string)?;
    let module = TcpTransportModule::new(config);
    Ok(Box::new(module))
}
