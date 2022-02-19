use thiserror::Error;

pub mod engine;
pub mod module;


#[derive(Debug, Error)]
pub(crate) enum Error {
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

#[derive(Error, Debug)]
pub(crate) enum DatapathError {
}
