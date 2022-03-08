use ipc::mrpc::{cmd, dp};
use ipc::service::ShmService;

use interface::engine::EngineType;

use crate::{Error as ServiceError, KOALA_PATH};

thread_local! {
    // Initialization is dynamically performed on the first call to with within a thread.
    pub(crate) static MRPC_CTX: Context = Context::register().expect("koala mRPC register failed");
}

pub(crate) struct Context {
    service: ShmService<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
}

impl Context {
    fn register() -> Result<Context, ServiceError> {
        let service = ShmService::register(KOALA_PATH, EngineType::Mrpc)?;
        Ok(Self { service })
    }
}

// mRPC library
pub mod stub;
pub mod shared_heap;
pub mod alloc;
pub mod codegen;

pub use thiserror::Error;
#[derive(Error, Debug)]
#[error("mrpc::Error")]
pub struct Error;


#[derive(Debug, Clone, Copy)]
pub struct Status;
