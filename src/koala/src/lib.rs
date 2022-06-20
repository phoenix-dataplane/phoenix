#![feature(peer_credentials_unix_socket)]
#![feature(ptr_metadata)]
#![feature(specialization)]
#![feature(type_alias_impl_trait)]
#![feature(generic_associated_types)]
#![feature(strict_provenance)]
#![feature(int_roundings)]
#![feature(ptr_internals)]
// message meta buffer
#![feature(maybe_uninit_uninit_array)]
#![feature(maybe_uninit_array_assume_init)]

#[macro_use]
extern crate tracing;
// alias
extern crate tracing as log;

pub mod config;
pub mod control;
pub mod engine;
pub mod mrpc;
pub mod node;
pub mod resource;
pub mod rpc_adapter;
pub mod salloc;
pub mod state_mgr;
pub mod transport;

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
