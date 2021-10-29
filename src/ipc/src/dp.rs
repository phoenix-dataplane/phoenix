//! Data path operations.
use serde::{Deserialize, Serialize};

use crate::interface::*;
use interface::*;

#[derive(Serialize, Deserialize)]
pub enum Request {
    PostRecv(Handle, u64, u64, u64, MemoryRegion),
    PostSend(Handle, u64, u64, u64, MemoryRegion, i32),
    Connect(Handle, Option<ConnParamOwned>),
    GetSendComp(Handle),
    GetRecvComp(Handle),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    PostRecv(Result<(), interface::Error>),
    PostSend(Result<(), interface::Error>),
    Connect(Result<(), interface::Error>),
    GetSendComp(Result<WorkCompletion,interface::Error>),
    GetRecvComp(Result<WorkCompletion,interface::Error>),
}
