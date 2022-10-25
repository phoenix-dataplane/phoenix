//! Data path operations.
use serde::{Deserialize, Serialize};

use uapi::Handle;
use uapi::net::{RemoteKey, SendFlags, WorkCompletion, CompletionQueue};
use uapi::buf::Range;

pub type WorkRequestSlot = [u8; 128];

// TODO(cjr): dedicate a channel for PollCq command.

#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum WorkRequest {
    PostRecv(Handle, u64, Range, Handle),
    PostSend(Handle, u64, Range, Handle, SendFlags),
    PostSendWithImm(Handle, u64, Range, Handle, SendFlags, u32),
    PostWrite(Handle, Handle, u64, Range, u64, RemoteKey, SendFlags),
    PostRead(Handle, Handle, u64, Range, u64, RemoteKey, SendFlags),
    PollCq(CompletionQueue),
}

pub type CompletionSlot = [u8; 128];

#[repr(C, align(64))]
#[derive(Debug)]
pub struct Completion {
    pub cq_handle: CompletionQueue,
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
