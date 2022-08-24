use std::io;

use fnv::FnvHashMap as HashMap;
use lazy_static::lazy_static;
use thiserror::Error;

use ipc::service::ShmService;
use ipc::transport::rdma::{cmd, dp};

use crate::{KOALA_CONTROL_SOCK, KOALA_PREFIX};

pub mod cm;
mod fp;
pub mod verbs;

// NOTE(cjr): Will lazy_static affect the performance?
lazy_static! {
    // A cq can be created by calling create_cq, but it can also come from create_ep
    pub(crate) static ref CQ_BUFFERS: spin::Mutex<HashMap<interface::CompletionQueue, verbs::CqBuffer>> =
        spin::Mutex::new(HashMap::default());
}

thread_local! {
    pub(crate) static KL_CTX: Context = Context::register().expect("koala transport register failed");
}

pub(crate) struct Context {
    service: ShmService<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
}

impl Context {
    fn register() -> Result<Context, Error> {
        let service = ShmService::register(
            &*KOALA_PREFIX,
            &*KOALA_CONTROL_SOCK,
            "RdmaTransport".to_string(),
        )?;
        Ok(Self { service })
    }
}

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
