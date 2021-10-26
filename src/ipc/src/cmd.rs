//! Control path commands.
use engine::SchedulingMode;
use serde::{Deserialize, Serialize};

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
    RegMsgs(Handle, u64, u64),
    PostRecv(Handle, u64, u64, u64, MemoryRegion),
    PostSend(Handle, u64, u64, u64, MemoryRegion, i32),
    Connect(Handle, Option<ConnParamOwned>),
    GetSendComp(Handle),
    GetRecvComp(Handle),
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
    GetSendComp(Result<WorkCompletion,interface::Error>),
    GetRecvComp(Result<WorkCompletion,interface::Error>),
}
