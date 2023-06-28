#![feature(peer_credentials_unix_socket)]
#![feature(ptr_internals)]
#![feature(strict_provenance)]
use thiserror::Error;

use crate::engine::struct_rpc_events_file;

use chrono::prelude::*;
use itertools::iproduct;
use rand::Rng;

pub use phoenix_common::{InitFnResult, PhoenixAddon};

pub mod config;
pub(crate) mod engine;
pub mod module;

#[derive(Error, Debug)]
pub(crate) enum DatapathError {
    #[error("Internal queue send error")]
    InternalQueueSend,
}

use phoenix_common::engine::datapath::SendError;
impl<T> From<SendError<T>> for DatapathError {
    fn from(_other: SendError<T>) -> Self {
        DatapathError::InternalQueueSend
    }
}

use crate::config::NofileLoggingConfig;
use crate::module::NofileLoggingAddon;

#[no_mangle]
pub fn init_addon(config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixAddon>> {
    let config = NofileLoggingConfig::new(config_string)?;
    let addon = NofileLoggingAddon::new(config);
    Ok(Box::new(addon))
}
