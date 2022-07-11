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

extern crate tracing;
// alias
extern crate tracing as log;

pub mod config;
pub mod control;
pub mod engine;
pub mod mrpc;
pub(crate) mod node;
pub(crate) mod resource;
pub(crate) mod rpc_adapter;
pub(crate) mod salloc;
pub(crate) mod state_mgr;
pub(crate) mod transport;

#[allow(unused)]
pub(crate) mod timer;

#[macro_export]
macro_rules! unimplemented_ungradable {
    ($engine:ident) => {
        use crate::engine::{Upgradable, Version};
        impl Upgradable for $engine {
            fn version(&self) -> Version {
                unimplemented!();
            }

            fn check_compatible(&self, _v2: Version) -> bool {
                unimplemented!();
            }

            fn suspend(&mut self) {
                unimplemented!();
            }

            fn dump(&self) {
                unimplemented!();
            }

            fn restore(&mut self) {
                unimplemented!();
            }
        }
    };
}
