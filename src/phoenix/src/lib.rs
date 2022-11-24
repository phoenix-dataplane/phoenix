#![feature(peer_credentials_unix_socket)]
#![feature(ptr_metadata)]
#![feature(type_alias_impl_trait)]
#![feature(strict_provenance)]
#![feature(int_roundings)]
#![feature(ptr_internals)]
#![feature(maybe_uninit_uninit_array)]
#![feature(maybe_uninit_array_assume_init)]
#![feature(core_intrinsics)]
#![feature(drain_filter)]

pub extern crate tracing;
// alias
pub extern crate tracing as log;

#[allow(clippy::missing_safety_doc)]
pub mod addon;
pub mod config;
pub mod control;
pub mod engine;
#[allow(clippy::missing_safety_doc)]
pub mod envelop;
pub mod local_resource;
#[allow(clippy::missing_safety_doc)]
pub mod module;
pub mod page_padded;
pub mod plugin;
pub mod resource;
pub mod state_mgr;
pub mod storage;
// pub mod transport_rdma;

// pub mod scheduler;

pub(crate) mod dependency;

#[allow(unused)]
pub mod timer;

pub use rdma;

pub type PhoenixResult<T> = anyhow::Result<T>;
