use std::io;

use thiserror::Error;

use interface::engine::EngineType;
use ipc::mrpc::{cmd, dp};
use ipc::service::ShmService;

use crate::{KOALA_CONTROL_SOCK, KOALA_PREFIX};

thread_local! {
    // Initialization is dynamically performed on the first call to with within a thread.
    pub(crate) static MRPC_CTX: Context = {
        &*crate::salloc::SA_CTX;
        Context::register().expect("koala mRPC register failed");
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
pub mod codegen;
pub mod stub;

// pub mod shmptr;

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
