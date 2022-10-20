use std::io;

use thiserror::Error;

use interface::engine::SchedulingHint;
use ipc::salloc::{cmd, dp};
use ipc::service::ShmService;

use libphoenix::{KOALA_CONTROL_SOCK, KOALA_PREFIX};

thread_local! {
    /// Initialization is dynamically performed on the first call to with within a thread.
    #[doc(hidden)]
    pub static SA_CTX: SAContext = SAContext::register().expect("phoenix salloc register failed");
}

pub struct SAContext {
    pub(crate) service:
        ShmService<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
}

impl SAContext {
    fn register() -> Result<SAContext, Error> {
        let service = ShmService::register(
            &*KOALA_PREFIX,
            &*KOALA_CONTROL_SOCK,
            "Salloc".to_string(),
            SchedulingHint::default(),
            None,
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
}
