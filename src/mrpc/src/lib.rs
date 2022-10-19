#![feature(negative_impls)]
#![feature(peer_credentials_unix_socket)]
#![feature(strict_provenance)]
#![feature(rustc_attrs)]
// rref
#![feature(maybe_uninit_uninit_array)]
#![feature(maybe_uninit_array_assume_init)]
// with_borrow_mut
#![feature(local_key_cell_methods)]
// WRef
#![feature(get_mut_unchecked)]

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::io;

use thiserror::Error;

pub use interface::engine::SchedulingHint;
use interface::Handle;
use ipc::mrpc::control_plane::Setting;
use ipc::mrpc::{cmd, dp};
use ipc::service::ShmService;
use libkoala::_rx_recv_impl as rx_recv_impl;
use libkoala::{KOALA_CONTROL_SOCK, KOALA_PREFIX};

pub use libkoala;

pub mod rheap;
pub use rheap::ReadHeap;

use salloc::backend::SA_CTX;

pub fn current_setting() -> Setting {
    SETTING.with_borrow(|s| s.clone())
}

pub fn set(setting: &Setting) {
    SETTING.with_borrow_mut(|s| *s = setting.clone());
}

pub fn get_schedulint_hint() -> SchedulingHint {
    SCHEDULING_HINT.with_borrow(|h| h.clone())
}

pub fn set_schedulint_hint(hint: &SchedulingHint) {
    SCHEDULING_HINT.with_borrow_mut(|h| *h = hint.clone());
}

thread_local! {
    pub(crate) static SETTING: RefCell<Setting> = RefCell::new(Setting::default());
    // Initialization is dynamically performed on the first call to with within a thread.
    pub(crate) static SCHEDULING_HINT: RefCell<SchedulingHint> = RefCell::new(Default::default());
    pub(crate) static MRPC_CTX: Context = {
        SA_CTX.with(|_ctx| {
            // do nothing, just to ensure SA_CTX is initialized before MRPC_CTX
        });
        Context::register(&current_setting()).expect("koala mRPC register failed")
    }
}

pub(crate) struct Context {
    protos: RefCell<BTreeSet<String>>,
    service: ShmService<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
}

impl Context {
    fn register(setting: &Setting) -> Result<Context, Error> {
        let protos = RefCell::new(BTreeSet::new());
        let setting_str = serde_json::to_string(setting)?;
        let service = ShmService::register(
            &*KOALA_PREFIX,
            &*KOALA_CONTROL_SOCK,
            "Mrpc".to_string(),
            SCHEDULING_HINT.with_borrow(|h| h.clone()),
            Some(&setting_str),
        )?;
        Ok(Self { protos, service })
    }

    fn update_protos(&self, protos: &[&str]) -> Result<(), Error> {
        let mut used_protos = self.protos.borrow_mut();
        let orig = used_protos.len();
        used_protos.extend(protos.iter().copied().map(String::from));
        if used_protos.len() > orig {
            let protos = used_protos.iter().cloned().collect::<Vec<_>>();
            let req = cmd::Command::UpdateProtos(protos);
            self.service.send_cmd(req)?;
            rx_recv_impl!(self.service, cmd::CompletionKind::UpdateProtos)?;
        }
        Ok(())
    }
}

// Re-exports shared memory collections and data types.
pub use shm::collections;
pub mod alloc {
    use salloc::SharedHeapAllocator;
    pub type Box<T> = shm::boxed::Box<T, SharedHeapAllocator>;
    pub type Vec<T> = shm::vec::Vec<T, SharedHeapAllocator>;
    pub type String = shm::string::String<SharedHeapAllocator>;
}

pub mod stub;
// pub mod stub2;

#[macro_use]
pub mod macros;

pub use interface::rpc::Token;

#[doc(hidden)]
pub use interface::rpc::MessageErased;

pub mod rref;
pub use rref::RRef;

pub mod wref;
pub use wref::{IntoWRef, WRef, WRefOpaque};

pub mod status;
pub use status::{Code, Status};

#[cfg(feature = "timing")]
pub(crate) mod timing;

pub mod sched;
pub use sched::{bind_to_node, num_numa_nodes};

/// A re-export of [`async-trait`](https://docs.rs/async-trait) for use with codegen.
pub use async_trait::async_trait;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Serde-json: {0:?}")]
    Serde(#[from] serde_json::Error),
    #[error("Service error: {0}")]
    Service(#[from] ipc::Error),
    #[error("IO Error {0}")]
    Io(#[from] io::Error),
    #[error("Interface error {0}: {1}")]
    Interface(&'static str, interface::Error),
    #[error("No address is resolved")]
    NoAddrResolved,
    #[error("Connect failed: {0}")]
    Connect(interface::Error),
    #[error("Disconnected: {0:?}")]
    Disconnect(Handle),
    #[error("Connection closed.")]
    ConnectionClosed,
}
