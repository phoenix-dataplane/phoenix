#![feature(ptr_internals)]
#![feature(strict_provenance)]
#![feature(local_key_cell_methods)]
#![feature(peer_credentials_unix_socket)]

use std::alloc::LayoutError;

use phoenix_salloc::region;
use phoenix_salloc::ControlPathError as SallocError;
use thiserror::Error;
use transport_tcp::{ops, ApiError, TransportError};

pub use phoenix::module::PhoenixModule;
pub use phoenix::plugin::InitFnResult;
use phoenix::resource::Error as ResourceError;

pub mod module;
pub mod state;

pub(crate) mod engine;
#[allow(unused)]
pub(crate) mod pool;
pub(crate) mod serialization;

#[inline]
fn get_ops() -> &'static ops::Ops {
    use crate::engine::ELS;
    ELS.with(|els| &els.borrow().as_ref().unwrap().ops)
}

#[derive(Error, Debug)]
#[error("tcp-rpc-adapter control path error")]
pub(crate) enum ControlPathError {
    // Below are errors that return to the user.
    #[error("Controlpath API error in socket: {0}")]
    ApiError(#[from] ApiError),
    #[error("TCP transport error: {0}")]
    TransportError(#[from] TransportError),
    #[error("Resource error: {0}")]
    Resource(#[from] ResourceError),
    #[error("Salloc error: {0}")]
    Salloc(#[from] SallocError),
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

    #[error("TCP transport error: {0}")]
    TransportError(#[from] TransportError),
}

use crate::module::TcpRpcAdapterModule;

#[no_mangle]
pub fn init_module(_config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixModule>> {
    let module = TcpRpcAdapterModule::new();
    Ok(Box::new(module))
}
