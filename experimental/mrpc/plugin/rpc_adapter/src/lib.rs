#![feature(ptr_internals)]
#![feature(strict_provenance)]
#![feature(local_key_cell_methods)]
#![feature(peer_credentials_unix_socket)]

//! this engine translate RPC messages into transport-level work requests / completions
use std::alloc::LayoutError;

use thiserror::Error;

use phoenix_salloc::region;

// Re-export PhoenixModule
pub use phoenix::module::PhoenixModule;
pub use phoenix::plugin::InitFnResult;
use phoenix::resource::Error as ResourceError;

pub mod module;
pub mod state;

pub(crate) mod acceptor;
pub mod config;
pub(crate) mod engine;
pub(crate) mod serialization;
pub(crate) mod ulib;

#[allow(unused)]
pub(crate) mod pool;

#[derive(Error, Debug)]
#[error("rpc-adapter control path error")]
pub(crate) enum ControlPathError {
    // Below are errors that return to the user.
    #[error("Ulib error {0}")]
    Ulib(#[from] ulib::Error),
    #[error("Resource error: {0}")]
    Resource(#[from] ResourceError),
    #[error("Invalid layout: {0}")]
    Layout(#[from] LayoutError),
    #[error("SharedRegion allocate error: {0}")]
    SharedRegion(#[from] region::Error),
    #[error("{0}")]
    InsertAddrMap(#[from] mrpc_marshal::AddressExists),

    // Below are errors that does not return to the user.
    #[error("Send command error")]
    SendCommand,
    #[error("Service error: {0}")]
    Service(#[from] ipc::Error),
    #[error("Loading dispatch library: {0}")]
    LibLoading(#[from] libloading::Error),
}

impl From<ControlPathError> for uapi::Error {
    fn from(other: ControlPathError) -> Self {
        uapi::Error::Generic(other.to_string())
    }
}

// use crate::engine::graph::SendError;
use phoenix::engine::datapath::EngineTxMessage;
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
    #[error("Tx queue send error: {0}")]
    Tx(#[from] phoenix::engine::datapath::SendError<EngineTxMessage>),
}

use crate::config::RpcAdapterConfig;
use crate::module::RpcAdapterModule;

#[no_mangle]
pub fn init_module(config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixModule>> {
    let config = RpcAdapterConfig::new(config_string)?;
    let module = RpcAdapterModule::new(config);
    Ok(Box::new(module))
}
