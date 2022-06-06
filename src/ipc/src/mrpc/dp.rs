//! mRPC data path operations.
use serde::{Deserialize, Serialize};

use interface::{rpc::MessageTemplateErased, Handle};

pub type WorkRequestSlot = [u8; 64];

#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum WorkRequest {
    Call(MessageTemplateErased),
    // this will also deallocate
    Reply(MessageTemplateErased),
}

pub type CompletionSlot = [u8; 64];

#[repr(C, align(64))]
#[derive(Debug)]
pub enum Completion {
    Recv(MessageTemplateErased),
    SendCompletion(Handle, u32)
}

mod sa {
    use super::*;
    use static_assertions::const_assert;
    use std::mem::size_of;
    const_assert!(size_of::<WorkRequest>() <= size_of::<WorkRequestSlot>());
    const_assert!(size_of::<Completion>() <= size_of::<CompletionSlot>());
}
