//! Data path operations.
use serde::{Deserialize, Serialize};

use interface::{Handle, SendFlags, WorkCompletion};

use crate::buf::Buffer;

pub type WorkRequestSlot = [u8; 64];

// TODO(cjr): dedicate a channel for PollCq command.

#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum WorkRequest {
    PostRecv(Handle, u64, Buffer, Handle),
    PostSend(Handle, u64, Buffer, Handle, SendFlags),
    PollCq(interface::CompletionQueue),
}

pub type CompletionSlot = [u8; 64];

#[repr(C, align(64))]
#[derive(Debug)]
pub struct Completion {
    pub cq_handle: interface::CompletionQueue,
    pub wc: WorkCompletion,
}

mod sa {
    use super::*;
    use std::mem::size_of;
    use static_assertions::const_assert;
    const_assert!(size_of::<WorkRequest>() <= size_of::<WorkRequestSlot>());
    const_assert!(size_of::<Completion>() <= size_of::<CompletionSlot>());
}
