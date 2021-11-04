//! Data path operations.
use serde::{Deserialize, Serialize};
use std::ops::Range;

use interface::{Handle, SendFlags, WorkCompletion};

type IResult<T> = Result<T, interface::Error>;

#[derive(Serialize, Deserialize)]
pub enum Request {
    PostRecv(Handle, u64, Range<u64>, Handle),
    PostSend(Handle, u64, Range<u64>, Handle, SendFlags),
    GetRecvComp(Handle),
    GetSendComp(Handle),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    PostRecv(IResult<()>),
    PostSend(IResult<()>),
    GetRecvComp(IResult<WorkCompletion>),
    GetSendComp(IResult<WorkCompletion>),
}
