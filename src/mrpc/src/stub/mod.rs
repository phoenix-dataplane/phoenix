use std::cell::RefCell;

use crate::{Error, MRPC_CTX};

/// Re-exports
pub use interface::rpc::{MessageErased, MessageMeta, RpcMsgType};
pub use ipc::mrpc::control_plane::TransportType;

pub mod service;
pub use service::{Service, NamedService, service_pre_handler, service_post_handler};

pub mod client;
pub use client::ClientStub;

pub mod server;
pub mod local_server;
pub use local_server::LocalServer;

pub mod reactor;
pub mod conn;
pub mod pending;
pub mod reply_cache;

// We can make RpcData a private trait, and only mark it for compiler generated types.
// This seems impossible.
pub trait RpcData: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> RpcData for T {}

// TODO(cjr): move this to mrpc::Context
pub use reactor::Reactor;
thread_local! {
    pub static LOCAL_REACTOR: RefCell<Reactor> = RefCell::new(Reactor::new());
}

pub fn update_protos(protos: &[&str]) -> Result<(), Error> {
    MRPC_CTX.with(|ctx| ctx.update_protos(protos))
}
