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

use interface::Handle;
use ipc::mrpc::{cmd, dp};
use ipc::service::ShmService;
use libkoala::_rx_recv_impl as rx_recv_impl;
use libkoala::{KOALA_CONTROL_SOCK, KOALA_PREFIX};

pub mod rheap;
pub use rheap::ReadHeap;

pub use salloc::backend::SA_CTX;

thread_local! {
    // Initialization is dynamically performed on the first call to with within a thread.
    pub(crate) static MRPC_CTX: Context = {
        SA_CTX.with(|_ctx| {
            // do nothing, just to ensure SA_CTX is initialized before MRPC_CTX
        });
        Context::register().expect("koala mRPC register failed")
    }
}

pub(crate) struct Context {
    protos: RefCell<BTreeSet<String>>,
    service: ShmService<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
}

impl Context {
    fn register() -> Result<Context, Error> {
        let protos = RefCell::new(BTreeSet::new());
        let service =
            ShmService::register(&*KOALA_PREFIX, &*KOALA_CONTROL_SOCK, "Mrpc".to_string())?;
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

#[macro_use]
pub mod macros;

pub use interface::rpc::Token;

#[doc(hidden)]
pub use interface::rpc::MessageErased;

pub mod rref;
pub use rref::RRef;

pub mod wref;
pub use wref::{IntoWRef, WRef};

pub mod status;
pub use status::{Code, Status};

#[cfg(feature = "timing")]
pub(crate) mod timing;

/// A re-export of [`async-trait`](https://docs.rs/async-trait) for use with codegen.
pub use async_trait::async_trait;

#[derive(Error, Debug)]
pub enum Error {
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
}
