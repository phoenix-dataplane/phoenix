//! this engine translate RPC messages into transport-level work requests / completions
use thiserror::Error;

use crate::resource::Error as ResourceError;
use crate::salloc::ControlPathError as SallocError;

pub(crate) mod acceptor;
pub(crate) mod engine;
pub(crate) mod module;
pub(crate) mod state;
pub(crate) mod ulib;

#[derive(Error, Debug)]
#[error("rpc-adapter control path error")]
pub(crate) enum ControlPathError {
    // Below are errors that return to the user.
    #[error("Ulib error {0}")]
    Ulib(#[from] ulib::Error),
    #[error("Resource error: {0}")]
    Resource(#[from] ResourceError),
    #[error("Salloc error: {0}")]
    Salloc(#[from] SallocError),

    // Below are errors that does not return to the user.
    #[error("Send command error")]
    SendCommand,
    #[error("Service error: {0}")]
    Service(#[from] ipc::Error),
}

impl From<ControlPathError> for interface::Error {
    fn from(other: ControlPathError) -> Self {
        interface::Error::Generic(other.to_string())
    }
}

use tokio::sync::mpsc::error::SendError;
impl<T> From<SendError<T>> for ControlPathError {
    fn from(_other: SendError<T>) -> Self {
        Self::SendCommand
    }
}

#[derive(Error, Debug)]
pub(crate) enum DatapathError {
    #[error("Resource error: {0}")]
    Resource(#[from] ResourceError),
    #[error("Ulib error {0}")]
    Ulib(#[from] ulib::Error),
}
