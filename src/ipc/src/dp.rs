//! Data path operations.
use serde::{Deserialize, Serialize};
use std::ops::Range;

use interface::{Handle, WorkCompletion, SendFlags};

type IResult<T> = Result<T, interface::Error>;

#[derive(Serialize, Deserialize)]
pub enum Request {
    PostRecv(Handle, u64, Range<u64>, Handle),
    PostSend(Handle, u64, Range<u64>, Handle, SendFlags),
    GetSendComp(Handle),
    GetRecvComp(Handle),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    PostRecv(IResult<()>),
    PostSend(IResult<()>),
    GetSendComp(IResult<WorkCompletion>),
    GetRecvComp(IResult<WorkCompletion>),
}
