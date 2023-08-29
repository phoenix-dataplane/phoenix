#![feature(ptr_internals)]
#![feature(peer_credentials_unix_socket)]

use thiserror::Error;

use phoenix_common::resource::Error as ResourceError;
pub use phoenix_common::{InitFnResult, PhoenixModule};

pub mod builder;
pub mod config;
pub(crate) mod engine;
// pub mod message;
// pub mod meta_pool;
pub mod module;
pub mod state;
pub mod unpack;

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
    #[error("Build marshal library failed: {0}")]
    MarshalLibBuilder(#[from] builder::Error),
}

impl From<Error> for phoenix_api::Error {
    fn from(other: Error) -> Self {
        phoenix_api::Error::Generic(other.to_string())
    }
}

#[derive(Error, Debug)]
pub(crate) enum DatapathError {
    #[error("Shared memory queue error: {0}.")]
    ShmIpc(#[from] ipc::shmem_ipc::ShmIpcError),
    #[error("Shared memory queue ringbuf error: {0}.")]
    ShmRingbuf(#[from] ipc::shmem_ipc::ShmRingbufError),
    #[error("Resource error: {0}")]
    Resource(#[from] ResourceError),
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

use phoenix_common::engine::datapath::SendError;
impl<T> From<SendError<T>> for DatapathError {
    fn from(_other: SendError<T>) -> Self {
        DatapathError::InternalQueueSend
    }
}

use crate::config::MrpcLBConfig;
use crate::module::MrpcLBModule;

#[no_mangle]
pub fn init_module(config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixModule>> {
    let config = MrpcLBConfig::new(config_string)?;
    let module = MrpcLBModule::new(config);
    Ok(Box::new(module))
}
