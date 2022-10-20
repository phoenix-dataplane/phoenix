use std::cell::RefCell;
use std::io;

use fnv::FnvHashMap as HashMap;
use lazy_static::lazy_static;
use thiserror::Error;

pub use interface::engine::SchedulingHint;
use ipc::service::ShmService;
use ipc::transport::rdma::control_plane::Setting;
use ipc::transport::rdma::{cmd, dp};

use crate::{KOALA_CONTROL_SOCK, KOALA_PREFIX};

pub mod cm;
mod fp;
pub mod verbs;

pub fn current_setting() -> Setting {
    SETTING.with_borrow(|s| s.clone())
}

pub fn set(setting: &Setting) {
    SETTING.with_borrow_mut(|s| *s = setting.clone());
}

pub fn set_schedulint_hint(hint: &SchedulingHint) {
    SCHEDULING_HINT.with_borrow_mut(|h| *h = hint.clone());
}

// NOTE(cjr): Will lazy_static affect the performance?
lazy_static! {
    // A cq can be created by calling create_cq, but it can also come from create_ep
    pub(crate) static ref CQ_BUFFERS: spin::Mutex<HashMap<interface::CompletionQueue, verbs::CqBuffer>> =
        spin::Mutex::new(HashMap::default());
}

thread_local! {
    pub(crate) static SETTING: RefCell<Setting> = RefCell::new(Setting::default());
    pub(crate) static SCHEDULING_HINT: RefCell<SchedulingHint> = RefCell::new(Default::default());
    pub(crate) static KL_CTX: Context = Context::register(&current_setting()).expect("phoenix transport register failed");
}

pub(crate) struct Context {
    service: ShmService<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
}

impl Context {
    fn register(setting: &Setting) -> Result<Context, Error> {
        let setting_str = serde_json::to_string(setting)?;
        let service = ShmService::register(
            &*KOALA_PREFIX,
            &*KOALA_CONTROL_SOCK,
            "RdmaTransport".to_string(),
            SCHEDULING_HINT.with_borrow(|h| h.clone()),
            Some(&setting_str),
        )?;
        Ok(Self { service })
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("Service error: {0}")]
    Service(#[from] ipc::Error),
    #[error("Serde-json: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("IO Error {0}")]
    Io(#[from] io::Error),
    #[error("Interface error {0}: {1}")]
    Interface(&'static str, interface::Error),
    #[error("No address is resolved")]
    NoAddrResolved,
    #[error("Connect failed: {0}")]
    Connect(interface::Error),
}
