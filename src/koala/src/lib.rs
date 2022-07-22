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

extern crate tracing;
// alias
extern crate tracing as log;

pub mod config;
pub mod control;
pub mod engine;
pub mod envelop;
pub mod module;
pub mod mrpc;
pub mod plugin;
pub mod resource;
pub mod state_mgr;
pub mod storage;

pub(crate) mod dependency;

pub(crate) mod node;
pub(crate) mod rpc_adapter;
pub(crate) mod salloc;
pub(crate) mod transport;

#[allow(unused)]
pub(crate) mod timer;

#[macro_export]
macro_rules! unimplemented_ungradable {
    ($engine:ident) => {
        use crate::engine::Unload;
        use crate::storage::{ResourceCollection, SharedStorage};
        impl Unload for $engine {
            fn detach(&mut self) {
                unimplemented!();
            }

            fn unload(
                self: Box<Self>,
                _shared: &mut SharedStorage,
                _global: &mut ResourceCollection,
            ) -> ResourceCollection {
                unimplemented!();
            }
        }
    };
}
