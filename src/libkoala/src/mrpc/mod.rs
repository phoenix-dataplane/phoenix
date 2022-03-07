use ipc::mrpc::{cmd, dp};
use ipc::service::ShmService;

use interface::engine::EngineType;

// Re-exports
use crate::{Error, KOALA_PATH};

pub mod stub;

thread_local! {
    // Initialization is dynamically performed on the first call to with within a thread.
    pub(crate) static MRPC_CTX: Context = Context::register().expect("koala mRPC register failed");
}

pub(crate) struct Context {
    service: ShmService<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
}

impl Context {
    fn register() -> Result<Context, Error> {
        let service = ShmService::register(KOALA_PATH, EngineType::Mrpc)?;
        Ok(Self { service })
    }
}