use ipc::mrpc::{cmd, dp};
use ipc::service::Service;

use interface::engine::EngineType;

// Re-exports
use crate::{Error, KOALA_PATH};

pub mod cm;

thread_local! {
    // Initialization is dynamically performed on the first call to with within a thread.
    pub(crate) static MRPC_CTX: Context = Context::register().expect("koala mRPC register failed");
}

pub(crate) struct Context {
    service: Service<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
}

impl Context {
    fn register() -> Result<Context, Error> {
        let service = Service::register(KOALA_PATH, EngineType::Mrpc)?;
        Ok(Self { service })
    }
}
