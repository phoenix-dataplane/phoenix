use thiserror::Error;

use ipc::customer::Error as CustomerError;

pub mod engine;
pub mod module;

#[derive(Debug, Error)]
pub(crate) enum Error {
    // Below are errors that return to the user.
    #[error("Failed to set transport type")]
    TransportType,

    // Below are errors that does not return to the user.
    #[error("ipc-channel TryRecvError")]
    IpcTryRecv,
    #[error("Customer error: {0}")]
    Customer(#[from] CustomerError),
}

impl From<Error> for interface::Error {
    fn from(other: Error) -> Self {
        interface::Error::Generic(other.to_string())
    }
}

#[derive(Error, Debug)]
pub(crate) enum DatapathError {}
