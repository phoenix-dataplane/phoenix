//! Socket data path operations.
//! COMMENT(cjr): What if we just provide the same set of interfaces as in rdma datapath?
use serde::{Deserialize, Serialize};

use phoenix_api::buf::Range;
use phoenix_api::net::{WcOpcode, WcStatus};
use phoenix_api::Handle;

pub type WorkRequestSlot = [u8; 64];

#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum WorkRequest {
    // NonblockingSend(Handle, Range, SendFlags),
    // NonblockingRecv(Handle, Range, RecvFlags),
    PostRecv(Handle, u64, Range),
    PostSend(Handle, u64, Range, u32),
    PollCq(Handle),
}

pub type CompletionSlot = [u8; 64];

#[repr(C, align(64))]
#[derive(Debug, Clone, Copy)]
pub struct Completion {
    pub wr_id: u64,
    pub conn_id: u64,
    pub opcode: WcOpcode,
    pub status: WcStatus,
    pub buf: Range,
    pub byte_len: usize,
    pub imm: u32,
}

mod sa {
    use super::*;
    use static_assertions::const_assert;
    use std::mem::size_of;
    const_assert!(size_of::<WorkRequest>() <= size_of::<WorkRequestSlot>());
    const_assert!(size_of::<Completion>() <= size_of::<CompletionSlot>());
}
