#![feature(peer_credentials_unix_socket)]
#![feature(local_key_cell_methods)]
#![feature(drain_filter)]

use thiserror::Error;

pub use phoenix::addon::PhoenixAddon;
pub use phoenix::plugin::InitFnResult;

pub mod config;
pub(crate) mod engine;
pub mod module;

#[derive(Error, Debug)]
pub(crate) enum DatapathError {
    #[error("Internal queue send error")]
    InternalQueueSend,
}

use phoenix::engine::datapath::SendError;
impl<T> From<SendError<T>> for DatapathError {
    fn from(_other: SendError<T>) -> Self {
        DatapathError::InternalQueueSend
    }
}
