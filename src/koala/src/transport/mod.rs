use std::io;
use thiserror::Error;

pub mod engine;
pub mod module;
pub mod resource;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("rdmacm internal error: {0}.")]
    RdmaCm(io::Error),
    #[error("ibv internal error: {0}.")]
    Ibv(io::Error),
    #[error("getaddrinfo error: {0}.")]
    GetAddrInfo(io::Error),
    #[error("Resource not found in table.")]
    NotFound,
    #[error("Resource exists in table.")]
    Exists,
    #[error("Cannot open or create shared memory file: {0}.")]
    ShmOpen(nix::Error),
    #[error("Failed to truncate file: {0}.")]
    Truncate(io::Error),
    #[error("Mmap failed: {0}.")]
    Mmap(nix::Error),
    #[error("Failed to send file descriptors: {0}.")]
    SendFd(ipc::unix::Error),
}

impl From<Error> for interface::Error {
    fn from(other: Error) -> Self {
        interface::Error::Generic(other.to_string())
    }
}

#[derive(Error, Debug)]
pub(crate) enum DatapathError {
    #[error("Resource not found in table.")]
    NotFound,
    #[error("User buffer out of the range.")]
    OutOfRange,
    #[error("Shared memory queue error: {0}.")]
    ShmIpc(#[from] ipc::ShmIpcError),
    #[error("Shared memory queue ringbuf error: {0}.")]
    ShmRingbuf(#[from] ipc::ShmRingbufError),
    #[error("rdmacm internal error: {0}.")]
    RdmaCm(io::Error),
    #[error("ibv internal error: {0}.")]
    Ibv(io::Error),
}

impl DatapathError {
    pub(crate) fn as_vendor_err(self) -> u32 {
        match self {
            Self::NotFound => 1024,
            Self::OutOfRange => 1025,
            Self::ShmIpc(_) => 1026,
            Self::ShmRingbuf(_) => 1027,
            Self::RdmaCm(e) => e.raw_os_error().unwrap() as u32,
            Self::Ibv(e) => e.raw_os_error().unwrap() as u32,
        }
    }
}
