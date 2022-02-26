use fnv::FnvHashMap as HashMap;

use lazy_static::lazy_static;

use interface::engine::EngineType;
use ipc::service::Service;
use ipc::transport::rdma::{cmd, dp};

// Re-exports
use crate::{Error, KOALA_PATH};

pub mod cm;
mod fp;
pub mod verbs;

// NOTE(cjr): Will lazy_static affects the performance?
lazy_static! {
    // A cq can be created by calling create_cq, but it can also come from create_ep
    pub(crate) static ref CQ_BUFFERS: spin::Mutex<HashMap<interface::CompletionQueue, verbs::CqBuffer>> =
        spin::Mutex::new(HashMap::default());
}

thread_local! {
    pub(crate) static KL_CTX: Context = Context::register().expect("koala transport register failed");
}

pub(crate) struct Context {
    service: Service<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
}

impl Context {
    fn register() -> Result<Context, Error> {
        let service = Service::register(KOALA_PATH, EngineType::RdmaTransport)?;
        Ok(Self { service })
    }
}
