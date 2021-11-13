//! Data path operations.
use serde::{Deserialize, Serialize};

use interface::{Handle, SendFlags, WorkCompletion};

use crate::buf::Buffer;

// type IResult<T> = Result<T, interface::Error>;

pub type WorkRequestSlot = [u8; 64];

// TODO(cjr): dedicate a channel for PollCq command.

#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum WorkRequest {
    PostRecv(Handle, u64, Buffer, Handle),
    PostSend(Handle, u64, Buffer, Handle, SendFlags),
    // GetRecvComp(Handle),
    // GetSendComp(Handle),
    PollCq(interface::CompletionQueue),
}

pub type CompletionSlot = [u8; 64];

#[repr(C, align(64))]
#[derive(Debug)]
pub struct Completion {
    pub cq_handle: interface::CompletionQueue,
    pub wc: WorkCompletion,
}

// #[repr(C)]
// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub enum CompletionResponse {
//     GetRecvComp(WorkCompletion),
//     GetSendComp(WorkCompletion),
//     // PollCq(Vec<WorkCompletion>),
// }

// #[derive(Debug, Serialize, Deserialize)]
// pub struct Response(pub IResult<ResponseKind>);

mod sa {
    use super::*;
    use std::mem::size_of;
    use static_assertions::const_assert;
    const_assert!(size_of::<WorkRequest>() <= size_of::<WorkRequestSlot>());
    const_assert!(size_of::<Completion>() <= size_of::<CompletionSlot>());
}
