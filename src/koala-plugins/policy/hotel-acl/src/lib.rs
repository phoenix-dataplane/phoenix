#![feature(peer_credentials_unix_socket)]
#![feature(ptr_internals)]
#![feature(strict_provenance)]

use thiserror::Error;

pub use koala::addon::KoalaAddon;
pub use koala::plugin::InitFnResult;

pub mod config;
pub(crate) mod engine;
pub mod module;

#[derive(Error, Debug)]
pub(crate) enum DatapathError {
    #[error("Internal queue send error")]
    InternalQueueSend,
}

use koala::engine::datapath::SendError;
impl<T> From<SendError<T>> for DatapathError {
    fn from(_other: SendError<T>) -> Self {
        DatapathError::InternalQueueSend
    }
}
