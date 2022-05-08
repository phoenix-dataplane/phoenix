use std::io;
use std::path::PathBuf;

use thiserror::Error;

use interface::engine::EngineType;
use ipc::mrpc::{cmd, dp};
use ipc::service::ShmService;

use crate::{DEFAULT_KOALA_PATH, DEFAULT_KOALA_CONTROL};

thread_local! {
    // Initialization is dynamically performed on the first call to with within a thread.
    pub(crate) static MRPC_CTX: Context = Context::register().expect("koala mRPC register failed");
}

pub(crate) struct Context {
    service: ShmService<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
}

impl Context {
    fn register() -> Result<Context, Error> {
        let koala_prefix = match std::env::var("KOALA_PATH") {
            Ok(path) => {
                let path = PathBuf::from(path);
                if !path.is_dir() {
                    return Err(Error::Io(io::Error::new(io::ErrorKind::NotFound, "KOALA_PATH is not a directory")));
                }
                path
            }
            Err(e) => {
                PathBuf::from(DEFAULT_KOALA_PATH)
            }
        };
        let koala_control = match std::env::var("KOALA_CONTROL") {
            Ok(path) => {
                PathBuf::from(path)
            }
            Err(e) => {
                PathBuf::from(DEFAULT_KOALA_CONTROL)
            }
        };

        let service = ShmService::register(koala_prefix, koala_control, EngineType::Mrpc)?;
        Ok(Self { service })
    }
}

// mRPC library
pub mod alloc;
pub mod codegen;
pub mod shared_heap;
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
