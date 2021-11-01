//! Control path commands.
use engine::SchedulingMode;
use serde::{Deserialize, Serialize};
use std::ops::Range;

use crate::interface::*;
use interface::*;

#[derive(Serialize, Deserialize)]
pub enum Request {
    NewClient(SchedulingMode),
    Hello(i32),

    CreateEp(
        Vec<interface::AddrInfo>,
        Option<Handle>,
        Option<QpInitAttrOwned>,
    ),
    RegMsgs(Handle, Range<u64>),
    Listen(Handle, i32),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    /// name of the OneShotServer
    NewClient(SchedulingMode, String),
    HelloBack(i32),

    // handle of cmid
    CreateEp(Result<Handle, interface::Error>), // TODO(lsh): Handle to CmIdOwned
    RegMsgs(Result<Handle, interface::Error>),
    Listen(Result<(), interface::Error>),
}
