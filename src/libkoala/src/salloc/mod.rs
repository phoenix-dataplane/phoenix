use std::io;

use lazy_static::lazy_static;
use thiserror::Error;

use interface::engine::EngineType;
use ipc::salloc::{cmd, dp};
use ipc::service::ShmService;

use crate::{KOALA_CONTROL_SOCK, KOALA_PREFIX};


thread_local! {
    // Initialization is dynamically performed on the first call to with within a thread.
    pub(crate) static SA_CTX: SAContext = SAContext::register().expect("koala salloc register failed");
}

lazy_static! {
    pub(crate) static ref GC_CTX: GCContext = GCContext::register();
}

pub(crate) struct GCContext;

impl GCContext {
    fn register() -> GCContext {
        lazy_static::initialize(&gc::GLOBAL_PAGE_POOL);
        // create an executor and put the empty page reclamation task on a dedicated thread
        let ex = async_executor::Executor::new();
        let task = gc::GLOBAL_PAGE_POOL.release_empty_pages();
        std::thread::spawn(move || smol::future::block_on(ex.run( async { 
            task.await
        })));
        GCContext
    }
}

pub(crate) struct SAContext {
    pub(crate) service:
        ShmService<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
}

impl SAContext {
    fn register() -> Result<SAContext, Error> {
        let service =
            ShmService::register(&*KOALA_PREFIX, &*KOALA_CONTROL_SOCK, EngineType::Salloc)?;
        Ok(Self { service })
    }
}

pub(crate) mod gc;
pub(crate) mod heap;
pub(crate) mod region;
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
