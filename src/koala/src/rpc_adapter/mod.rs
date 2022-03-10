//! this engine translate RPC messages into transport-level work requests / completions
use thiserror::Error;

pub mod engine;
pub mod module;
pub mod state;
pub mod ulib;

#[derive(Error, Debug)]
#[error("rpc-adapter control path error")]
pub(crate) enum ControlPathError {
    // Below are errors that return to the user.
    #[error("Interface error {0}: {1}")]
    Interface(&'static str, interface::Error),
    #[error("Ulib error {0}")]
    Ulib(#[from] ulib::Error),

    // Below are errors that does not return to the user.
    #[error("Operation in progress")]
    InProgress,
    #[error("Ipc-channel TryRecvError")]
    IpcTryRecv,
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

use std::sync::mpsc::SendError;
impl<T> From<SendError<T>> for ControlPathError {
    fn from(other: SendError<T>) -> Self {
        Self::SendCommand
    }
}

#[derive(Error, Debug)]
#[error("rpc-adapter datapath error")]
pub(crate) enum DatapathError {
}
