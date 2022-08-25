#![feature(peer_credentials_unix_socket)]
#![feature(ptr_metadata)]
#![feature(type_alias_impl_trait)]
#![feature(generic_associated_types)]
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

pub mod addon;
pub mod config;
pub mod control;
pub mod engine;
pub mod envelop;
pub mod module;
pub mod plugin;
pub mod resource;
pub mod state_mgr;
pub mod storage;

pub(crate) mod dependency;

#[allow(unused)]
pub(crate) mod timer;
