#![feature(ptr_internals)]
#![feature(strict_provenance)]

use std::path::Path;

use thiserror::Error;

use koala::module::KoalaModule;
use koala::resource::Error as ResourceError;

use salloc::ControlPathError as SallocError;

pub(crate) mod acceptor;
pub(crate) mod engine;
pub mod module;
pub(crate) mod serialization;
pub mod state;
pub(crate) mod ulib;

use module::RpcAdapterModule;

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
    #[error("Loading dispatch library: {0}")]
    LibLoading(#[from] libloading::Error),
}

impl From<ControlPathError> for interface::Error {
    fn from(other: ControlPathError) -> Self {
        interface::Error::Generic(other.to_string())
    }
}

// use crate::engine::graph::SendError;
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

#[no_mangle]
pub fn init_module(_config_path: Option<&Path>) -> Box<dyn KoalaModule> {
    let module = RpcAdapterModule::new();
    Box::new(module)
}
