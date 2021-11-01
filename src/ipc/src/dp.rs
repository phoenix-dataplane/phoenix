//! Data path operations.
use serde::{Deserialize, Serialize};
use std::ops::Range;

use crate::interface::*;
use interface::*;

#[derive(Serialize, Deserialize)]
pub enum Request {
    PostRecv(Handle, u64, Range<u64>, Handle),
    PostSend(Handle, u64, Range<u64>, Handle, i32),
    Connect(Handle, Option<ConnParamOwned>),
    GetSendComp(Handle),
    GetRecvComp(Handle),
    GetRequest(Handle),
    Accept(Handle, Option<ConnParamOwned>),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    PostRecv(Result<(), interface::Error>),
    PostSend(Result<(), interface::Error>),
    Connect(Result<(), interface::Error>),
    GetSendComp(Result<WorkCompletion, interface::Error>),
    GetRecvComp(Result<WorkCompletion, interface::Error>),
    GetRequest(Result<Handle, interface::Error>),
    Accept(Result<(), interface::Error>),
}
