//! Data path operations.
use serde::{Deserialize, Serialize};

use interface::{Handle, RemoteKey, SendFlags, WorkCompletion};

use crate::buf::Range;

pub type WorkRequestSlot = [u8; 64];

// TODO(cjr): dedicate a channel for PollCq command.

#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum WorkRequest {
    PostRecv(Handle, u64, Range, Handle),
    PostSend(Handle, u64, Range, Handle, SendFlags),
    PostWrite(Handle, Handle, u64, Range, u64, RemoteKey, SendFlags),
    PostRead(Handle, Handle, u64, Range, u64, RemoteKey, SendFlags),
    PollCq(interface::CompletionQueue),
    VerbsPostSendFirst(interface::VerbsRequestFirst),
    VerbsPostSendSecond(interface::VerbsRequestSecond),
}

pub type CompletionSlot = [u8; 64];

#[repr(C, align(64))]
#[derive(Debug)]
pub struct Completion {
    pub cq_handle: interface::CompletionQueue,
    pub _padding: [u8; 4],
    pub wc: WorkCompletion,
}

mod sa {
    use super::*;
    use static_assertions::const_assert;
    use std::mem::size_of;
    const_assert!(size_of::<WorkRequest>() <= size_of::<WorkRequestSlot>());
    const_assert!(size_of::<Completion>() <= size_of::<CompletionSlot>());
}
