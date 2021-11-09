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
    PollCq(interface::CompletionQueue, usize),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ResponseKind {
    PostRecv,
    PostSend,
    GetRecvComp(WorkCompletion),
    GetSendComp(WorkCompletion),
    PollCq(Vec<WorkCompletion>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);
