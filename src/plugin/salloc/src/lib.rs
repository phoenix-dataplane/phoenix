#![feature(int_roundings)]
#![feature(strict_provenance)]
#![feature(peer_credentials_unix_socket)]
#![feature(local_key_cell_methods)]

use std::alloc::LayoutError;

use thiserror::Error;

use phoenix::module::PhoenixModule;
use phoenix::plugin::InitFnResult;
use phoenix::resource::Error as ResourceError;

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

impl From<ControlPathError> for uapi::Error {
    fn from(other: ControlPathError) -> Self {
        uapi::Error::Generic(other.to_string())
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

use std::cell::RefCell;
thread_local! {
    static my_tls: RefCell<i32> = RefCell::new(1);
    static my_tls2: RefCell<i32> = RefCell::new(2);
}

#[no_mangle]
pub fn init_module_salloc(config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixModule>> {
    let config = SallocConfig::new(config_string)?;
    // let config = SallocConfig {
    //     prefix: Some("/tmp/phoenix".into()),
    //     engine_basename: "salloc".to_owned(),
    // };
    let module = SallocModule::new(config);
    Ok(Box::new(module))
}

#[no_mangle]
pub fn init_module_salloc2(a: i32, b: i32) -> i32 {
    println!("{} {}", a, b);
    my_tls.with_borrow_mut(|addend1| {
        *addend1 += 1;
        my_tls2.with_borrow_mut(|addend| {
            println!("addend1: {}, addend: {}", addend1, addend);
            *addend += 2;
            a + b + *addend1 + *addend
        })
    })
    // a + b
}

#[no_mangle]
pub fn init_module(config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixModule>> {
    let config = SallocConfig::new(config_string)?;
    let module = SallocModule::new(config);
    Ok(Box::new(module))
}
