use std::io;
use thiserror::Error;

pub mod engine;
pub mod module;

#[derive(Debug, Error)]
enum Error {
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
    #[error("Cannot open or create shared memory file: {0}")]
    ShmOpen(nix::Error),
    #[error("Failed to truncate file: {0}")]
    Truncate(io::Error),
    #[error("Mmap failed: {0}")]
    Mmap(nix::Error),
    #[error("Failed to send file descriptors: {0}")]
    SendFd(io::Error),
}

impl From<Error> for interface::Error {
    fn from(other: Error) -> Self {
        interface::Error::Generic(other.to_string())
    }
}
