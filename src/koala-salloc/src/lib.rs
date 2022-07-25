#![feature(int_roundings)]
#![feature(strict_provenance)]
#![feature(peer_credentials_unix_socket)]

use std::{alloc::LayoutError, path::Path};

use thiserror::Error;

use koala::module::KoalaModule;
use koala::resource::Error as ResourceError;

pub(crate) mod config;
pub(crate) mod engine;
pub mod module;
pub mod region;
pub mod state;

use config::SallocConfig;
use module::SallocModule;

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

impl From<ControlPathError> for interface::Error {
    fn from(other: ControlPathError) -> Self {
        interface::Error::Generic(other.to_string())
    }
}

use std::sync::mpsc::SendError;
impl<T> From<SendError<T>> for ControlPathError {
    fn from(_other: SendError<T>) -> Self {
        Self::SendCommand
    }
}

#[no_mangle]
pub fn init_module(config_path: Option<&Path>) -> Box<dyn KoalaModule> {
    let config = if let Some(path) = config_path {
        SallocConfig::from_path(path).unwrap()
    } else {
        SallocConfig::default()
    };
    let module = SallocModule::new(config);
    Box::new(module)
}
