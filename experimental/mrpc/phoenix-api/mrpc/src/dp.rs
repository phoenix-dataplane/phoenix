//! mRPC data path operations.
use serde::{Deserialize, Serialize};

use phoenix_api::rpc::{CallId, MessageErased, RpcId, TransportStatus};
use phoenix_api::Handle;

pub type WorkRequestSlot = [u8; 64];

pub const RECV_RECLAIM_BS: usize = 4;

#[repr(C, align(64))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum WorkRequest {
    Call(MessageErased),
    // this will also deallocate
    Reply(MessageErased),
    // conn_id and an array of call_id
    ReclaimRecvBuf(Handle, [CallId; RECV_RECLAIM_BS]),
}

pub type CompletionSlot = [u8; 64];

// Avoid using too much `Send`/`Recv` in the code.
#[repr(C, align(64))]
#[derive(Debug, Clone)]
pub enum Completion {
    Incoming(MessageErased),
    Outgoing(RpcId, TransportStatus),
    // (conn_id, status)
    RecvError(Handle, TransportStatus),
}

mod sa {
    use super::*;
    use static_assertions::const_assert;
    use std::mem::size_of;
    const_assert!(size_of::<WorkRequest>() <= size_of::<WorkRequestSlot>());
    const_assert!(size_of::<Completion>() <= size_of::<CompletionSlot>());
}
