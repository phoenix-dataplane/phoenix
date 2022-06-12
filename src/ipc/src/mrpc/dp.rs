//! mRPC data path operations.
use serde::{Deserialize, Serialize};

use interface::{rpc::MessageErased, Handle};

#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct WRIdentifier(pub Handle, pub u32);

pub type WorkRequestSlot = [u8; 64];

#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum WorkRequest {
    Call(MessageErased),
    // this will also deallocate
    Reply(MessageErased),
}

pub type CompletionSlot = [u8; 64];

#[repr(C, align(64))]
#[derive(Debug)]
pub enum Completion {
    Recv(MessageErased),
    SendCompletion(Handle, u32),
}

mod sa {
    use super::*;
    use static_assertions::const_assert;
    use std::mem::size_of;
    const_assert!(size_of::<WorkRequest>() <= size_of::<WorkRequestSlot>());
    const_assert!(size_of::<Completion>() <= size_of::<CompletionSlot>());
}
