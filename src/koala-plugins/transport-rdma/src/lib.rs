#![feature(strict_provenance)]
#![feature(peer_credentials_unix_socket)]

use thiserror::Error;

pub use koala::module::KoalaModule;
pub use koala::plugin::InitFnResult;
use koala::transport_rdma::{ApiError, DatapathError};

pub(crate) mod cm;
pub mod config;
pub(crate) mod engine;
pub mod module;

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
