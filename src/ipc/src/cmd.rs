//! Control path commands.
use engine::SchedulingMode;
use serde::{Deserialize, Serialize};

use crate::interface::QpInitAttrOwned;
use interface::Handle;

#[derive(Serialize, Deserialize)]
pub enum Request {
    NewClient(SchedulingMode),
    Hello(i32),

    CreateEp(
        Vec<interface::AddrInfo>,
        Option<Handle>,
        Option<QpInitAttrOwned>,
    ),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    /// name of the OneShotServer
    NewClient(SchedulingMode, String),
    HelloBack(i32),

    // handle of cmid
    CreateEp(Result<Handle, interface::Error>),
}
