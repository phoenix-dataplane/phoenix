#![feature(int_roundings)]
#![feature(strict_provenance)]
#![feature(peer_credentials_unix_socket)]

use std::alloc::LayoutError;

use thiserror::Error;

use phoenix_common::resource::Error as ResourceError;
use phoenix_common::{InitFnResult, PhoenixModule};

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

impl From<ControlPathError> for phoenix_api::Error {
    fn from(other: ControlPathError) -> Self {
        phoenix_api::Error::Generic(other.to_string())
    }
}

use std::sync::mpsc::SendError;
impl<T> From<SendError<T>> for ControlPathError {
    fn from(_other: SendError<T>) -> Self {
        Self::SendCommand
    }
}

use crate::config::SallocConfig;
use crate::module::SallocModule;

#[no_mangle]
pub fn init_module(config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixModule>> {
    let config = SallocConfig::new(config_string)?;
    let module = SallocModule::new(config);
    Ok(Box::new(module))
}
