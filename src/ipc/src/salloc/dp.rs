//! salloc data path operations.
use serde::{Deserialize, Serialize};

pub type WorkRequestSlot = [u8; 64];

#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum WorkRequest {
    Stub,
}

pub type CompletionSlot = [u8; 64];

#[repr(C, align(64))]
#[derive(Debug)]
pub struct Completion {
}

mod sa {
    use super::*;
    use static_assertions::const_assert;
    use std::mem::size_of;
    const_assert!(size_of::<WorkRequest>() <= size_of::<WorkRequestSlot>());
    const_assert!(size_of::<Completion>() <= size_of::<CompletionSlot>());
}
