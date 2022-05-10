#![feature(peer_credentials_unix_socket)]
#![feature(ptr_metadata)]
#![feature(specialization)]
#![feature(type_alias_impl_trait)]
#![feature(generic_associated_types)]
#![feature(strict_provenance)]

#[macro_use]
extern crate log;

pub mod engine;
pub mod resource;
pub mod state_mgr;
pub mod transport;
pub mod mrpc;
pub mod rpc_adapter;
pub mod salloc;
pub mod control;
pub mod config;
pub mod node;

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
    }
}
