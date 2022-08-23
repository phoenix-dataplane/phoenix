use std::io;

use thiserror::Error;

use ipc::salloc::{cmd, dp};
use ipc::service::ShmService;

use crate::{KOALA_CONTROL_SOCK, KOALA_PREFIX};

thread_local! {
    // Initialization is dynamically performed on the first call to with within a thread.
    pub(crate) static SA_CTX: SAContext = SAContext::register().expect("koala salloc register failed");
}

pub(crate) struct SAContext {
    pub(crate) service:
        ShmService<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
}

impl SAContext {
    fn register() -> Result<SAContext, Error> {
        let service =
            ShmService::register(&*KOALA_PREFIX, &*KOALA_CONTROL_SOCK, "Salloc".to_string())?;
        Ok(Self { service })
    }
}

pub(crate) mod gc;
pub(crate) mod heap;
pub(crate) mod region;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Service error: {0}")]
    Service(#[from] ipc::Error),
    #[error("IO Error {0}")]
    Io(#[from] io::Error),
    #[error("Interface error {0}: {1}")]
    Interface(&'static str, interface::Error),
}
