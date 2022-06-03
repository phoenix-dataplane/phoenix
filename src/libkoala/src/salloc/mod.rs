use std::io;

use thiserror::Error;

use interface::engine::EngineType;
use ipc::salloc::{cmd, dp};
use ipc::service::ShmService;

use crate::{KOALA_CONTROL_SOCK, KOALA_PREFIX};

thread_local! {
    // Initialization is dynamically performed on the first call to with within a thread.
    pub(crate) static SA_CTX: Context = Context::register().expect("koala salloc register failed");
}

pub(crate) struct Context {
    pub(crate) service: ShmService<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
}

impl Context {
    fn register() -> Result<Context, Error> {
        let service =
            ShmService::register(&*KOALA_PREFIX, &*KOALA_CONTROL_SOCK, EngineType::Salloc)?;
        Ok(Self { service })
    }
}

pub(crate) mod heap;
pub(crate) mod region;
pub(crate) mod gc;
// TODO(wyj): wrap the O generic and change to pub(crate)
pub(crate) mod owner;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Service error: {0}")]
    Service(#[from] ipc::Error),
    #[error("IO Error {0}")]
    Io(#[from] io::Error),
    #[error("Interface error {0}: {1}")]
    Interface(&'static str, interface::Error),
}
