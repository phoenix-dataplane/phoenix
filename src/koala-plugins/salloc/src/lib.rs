#![feature(int_roundings)]
#![feature(strict_provenance)]
#![feature(peer_credentials_unix_socket)]

use std::alloc::LayoutError;

use thiserror::Error;

pub use koala::module::KoalaModule;
pub use koala::plugin::InitFnResult;
use koala::resource::Error as ResourceError;

pub mod config;
pub(crate) mod engine;
pub mod module;
pub mod region;
pub mod state;

#[derive(Error, Debug)]
pub enum ControlPathError {
    // Below are errors that return to the user.
    #[error("Resource error: {0}")]
    Resource(#[from] ResourceError),
    #[error("Invalid layout: {0}")]
    Layout(#[from] LayoutError),
    #[error("SharedRegion allocate error: {0}")]
    SharedRegion(#[from] region::Error),
    // Below are errors that does not return to the user.
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
    fn from(_other: SendError<T>) -> Self {
        Self::SendCommand
    }
}
