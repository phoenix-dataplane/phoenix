#![feature(peer_credentials_unix_socket)]
#![feature(ptr_internals)]
#![feature(strict_provenance)]
use thiserror::Error;

use crate::engine::struct_acl;

use chrono::prelude::*;
use itertools::iproduct;

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

use crate::config::HelloAclConfig;
use crate::module::HelloAclAddon;

#[no_mangle]
pub fn init_addon(config_string: Option<&str>) -> InitFnResult<Box<dyn PhoenixAddon>> {
    let config = HelloAclConfig::new(config_string)?;
    let addon = HelloAclAddon::new(config);
    Ok(Box::new(addon))
}
