#![feature(negative_impls)]
#![feature(peer_credentials_unix_socket)]
#![feature(allocator_api)]
#![feature(nonnull_slice_from_raw_parts)]
#![feature(min_specialization)]
#![feature(strict_provenance)]
// boxed.rs
// TODO: clean up
#![feature(exact_size_is_empty)]
#![feature(ptr_internals)]
#![feature(ptr_metadata)]
#![feature(core_intrinsics)]
#![feature(ptr_const_cast)]
#![feature(try_reserve_kind)]
#![feature(trusted_len)]
#![feature(extend_one)]
#![feature(rustc_attrs)]
#![feature(slice_ptr_get)]
#![feature(slice_ptr_len)]
// GC
#![feature(drain_filter)]
// stub
#![feature(hash_drain_filter)]
// shmview
#![feature(maybe_uninit_uninit_array)]
#![feature(maybe_uninit_array_assume_init)]

use std::io;

use thiserror::Error;

use interface::engine::EngineType;
use ipc::mrpc::{cmd, dp};
use ipc::service::ShmService;
use libkoala::{KOALA_CONTROL_SOCK, KOALA_PREFIX};

thread_local! {
    // Initialization is dynamically performed on the first call to with within a thread.
    pub(crate) static MRPC_CTX: Context = {
        crate::salloc::SA_CTX.with(|_ctx| {
            // do nothing, just to ensure SA_CTX is initialized before MRPC_CTX
        });
        Context::register().expect("koala mRPC register failed")
    }
}

pub(crate) struct Context {
    service: ShmService<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
}

impl Context {
    fn register() -> Result<Context, Error> {
        let service = ShmService::register(&*KOALA_PREFIX, &*KOALA_CONTROL_SOCK, EngineType::Mrpc)?;
        Ok(Self { service })
    }
}

// mRPC library
pub mod alloc;
// pub mod codegen;  // use include! macro
pub mod stub;

// TODO(wyj): change to pub(crate)
pub mod salloc;

#[macro_use]
pub mod macros;

pub use interface::rpc::MessageErased;

pub mod shmview;

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
}

#[derive(Debug, Clone, Copy)]
pub struct Status;
