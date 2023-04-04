//! The first implementation of RPC-as-a-Service architecture.
//!
//! [`mRPC`] is an implementation of RPC-as-a-Service architecture which removes redundant
//! (un)marshalling steps in tradition library + sidecar approaches while achieving manageability to
//! RPCs. It contains a frontend shim library that is linked with user apps and a backend service
//! that runs as a plugin on [`Phoenix`]. This crate is the frontend shim library.
//!
//! The API design goal of this crate is to have a very similar developing experience as [`tonic`],
//! which is a popular [gRPC] implementation in Rust. Migrating applications that are written in `tonic` is
//! expected to require minimal effort.
//!
//! # Examples
//!
//! Examples can be found in the [`mrpc-examples`].
//!
//! # Getting Started
//!
//! mRPC service must be used with [`Phoenix`], so follow the instructions in the
//! [`phoenix-readme`] to set up the pheonix dataplane.
//!
//! Follow the instructions in the [`mrpc-tutorial`] to learn how to write applications.
//!
//! # Structure
//! <div>
//! <img src="../../../mrpc-overview.png" height="400" width="600" />
//! </div>
//! <hr/>
//!
//! # Max Message Size
//!
//! Currently, both servers and clients are using a fixed `8MB` as the limit for maximal message size.
//! This fixed limit will be removed or made configurable in the future.
//!
//! [`mRPC`]: https://github.com/phoenix-dataplane/phoenix/tree/main/experimental/mrpc
//! [`Phoenix`]: https://github.com/phoenix-dataplane/phoenix
//! [`mrpc-examples`]: https://github.com/phoenix-dataplane/phoenix/tree/main/experimental/mrpc/examples
//! [`mrpc-tutorial`]: https://phoenix-dataplane.github.io/tutorials/working-with-mrpc-library.html
//! [`phoenix-readme`]: https://github.com/phoenix-dataplane/phoenix/blob/main/README.md
//! [`tonic`]: https://github.com/hyperium/tonic
//! [gRPC]: https://grpc.io

#![warn(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
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

use ipc::service::ShmService;
pub use phoenix_api::engine::SchedulingHint;
use phoenix_api::Handle;
use phoenix_api_mrpc::control_plane::Setting;
use phoenix_api_mrpc::{cmd, dp};
use phoenix_syscalls::_rx_recv_impl as rx_recv_impl;
use phoenix_syscalls::{PHOENIX_CONTROL_SOCK, PHOENIX_PREFIX};

pub use phoenix_syscalls;

pub mod rheap;
#[doc(inline)]
pub use rheap::ReadHeap;

use shmalloc::backend::SA_CTX;

/// Returns the current mRPC [`Setting`].
pub fn current_setting() -> Setting {
    SETTING.with_borrow(|s| s.clone())
}

/// Update the current [`Setting`] to the given value.
///
/// # Note
///
/// This API must be called before any other mRPC APIs to make it effective.
pub fn set(setting: &Setting) {
    SETTING.with_borrow_mut(|s| *s = setting.clone());
}

/// Returns the current mRPC [`SchedulingHint`].
pub fn get_schedulint_hint() -> SchedulingHint {
    SCHEDULING_HINT.with_borrow(|h| *h)
}

/// Update the current [`SchedulingHint`] to the given value.
///
/// # Note
///
/// This API must be called before any other phoenix APIs to make it effective.
pub fn set_schedulint_hint(hint: &SchedulingHint) {
    SCHEDULING_HINT.with_borrow_mut(|h| *h = *hint);
}

thread_local! {
    pub(crate) static SETTING: RefCell<Setting> = RefCell::new(Setting::default());
    // Initialization is dynamically performed on the first call to with within a thread.
    pub(crate) static SCHEDULING_HINT: RefCell<SchedulingHint> = RefCell::new(Default::default());
    pub(crate) static MRPC_CTX: Context = {
        SA_CTX.with(|_ctx| {
            // do nothing, just to ensure SA_CTX is initialized before MRPC_CTX
        });
        Context::register(&current_setting()).expect("phoenix mRPC register failed")
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
            &*PHOENIX_PREFIX,
            &*PHOENIX_CONTROL_SOCK,
            "Mrpc".to_string(),
            SCHEDULING_HINT.with_borrow(|h| *h),
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

/// Re-exports shared memory collections and data types.
#[doc(inline)]
pub use shm::collections;

/// Re-exports shared memory collections and data types.
pub mod alloc {
    use shmalloc::SharedHeapAllocator;
    /// Shared memory Box whose memory is managed by [`SharedHeapAllocator`].
    pub type Box<T> = shm::boxed::Box<T, SharedHeapAllocator>;
    /// Shared memory Vec whose memory is managed by [`SharedHeapAllocator`].
    pub type Vec<T> = shm::vec::Vec<T, SharedHeapAllocator>;
    /// Shared memory String whose memory is managed by [`SharedHeapAllocator`].
    pub type String = shm::string::String<SharedHeapAllocator>;
}

pub mod stub;

#[macro_use]
mod macros;

#[doc(inline)]
pub use phoenix_api::rpc::Token;

#[doc(hidden)]
pub use phoenix_api::rpc::MessageErased;

mod rref;
pub use rref::RRef;

mod wref;
pub use wref::{IntoWRef, WRef, WRefOpaque};

mod status;
#[doc(inline)]
pub use status::{Code, Status};

#[cfg(feature = "timing")]
pub(crate) mod timing;

pub mod sched;
#[doc(inline)]
pub use sched::{bind_to_node, num_numa_nodes};

/// A re-export of [`async-trait`](https://docs.rs/async-trait) for use with codegen.
pub use async_trait::async_trait;

/// The error type for operations interacting with the mRPC service.
#[derive(Error, Debug)]
pub enum Error {
    #[error("Serde-json: {0:?}")]
    Serde(#[from] serde_json::Error),
    #[error("Service error: {0}")]
    Service(#[from] ipc::Error),
    #[error("IO Error {0}")]
    Io(#[from] io::Error),
    #[error("Interface error {0}: {1}")]
    Interface(&'static str, phoenix_api::Error),
    #[error("No address is resolved")]
    NoAddrResolved,
    #[error("Connect failed: {0}")]
    Connect(phoenix_api::Error),
    #[error("Disconnected: {0:?}")]
    Disconnect(Handle),
    #[error("Connection closed.")]
    ConnectionClosed,
}
