//! Control path commands.
use engine::SchedulingMode;
use serde::{Deserialize, Serialize};

use crate::interface::{ConnParamOwned, QpInitAttrOwned};
use interface::{CmId, Handle, IbvMr};

#[derive(Serialize, Deserialize)]
pub enum Request {
    NewClient(SchedulingMode),
    Hello(i32),

    CreateEp(
        Vec<interface::AddrInfo>,
        Option<Handle>,
        Option<QpInitAttrOwned>,
    ),
    RegMsgs(Handle, u64, u64),
    PostRecv(Handle, u64, u64, u64, IbvMr),
    PostSend(Handle, u64, u64, u64, IbvMr, i32),
    Connect(Handle, Option<ConnParamOwned>),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    /// name of the OneShotServer
    NewClient(SchedulingMode, String),
    HelloBack(i32),

    // handle of cmid
    CreateEp(Result<Handle, interface::Error>), // TODO(lsh): Handle to CmIdOwned
    RegMsgs(Result<Handle, interface::Error>),
    PostRecv(Result<(), interface::Error>),
    PostSend(Result<(), interface::Error>),
    Connect(Result<(), interface::Error>),
}
