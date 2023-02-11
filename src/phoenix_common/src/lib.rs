#![feature(peer_credentials_unix_socket)]
#![feature(ptr_internals)]

pub extern crate tracing;
// alias
pub extern crate tracing as log;

#[allow(clippy::missing_safety_doc)]
pub mod addon;
#[allow(clippy::missing_safety_doc)]
pub mod module;

pub mod engine;
#[allow(clippy::missing_safety_doc)]
pub mod envelop;
pub mod local_resource;

pub mod page_padded;
pub mod resource;
pub mod state_mgr;
pub mod storage;

#[allow(unused)]
pub mod timer;

pub type PhoenixResult<T> = anyhow::Result<T>;

// Re-export for plugin implementer's use
pub type InitFnResult<T> = anyhow::Result<T>;
pub use addon::PhoenixAddon;
pub use module::PhoenixModule;

// Takes an optional configuration string.
pub type InitAddonFn = fn(Option<&str>) -> InitFnResult<Box<dyn PhoenixAddon>>;
pub type InitModuleFn = fn(Option<&str>) -> InitFnResult<Box<dyn PhoenixModule>>;
