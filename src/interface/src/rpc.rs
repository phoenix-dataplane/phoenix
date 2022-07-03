//! RPC data structures
use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::Handle;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RpcMsgType {
    Request,
    Response,
}

/// We pack `conn_id` and `call_id` into `RpcId`. It is used it uniquely identify an RPC call.
#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RpcId(pub Handle, pub u32);

/// Transport layer status.
///
/// This transport error will be translated to mrpc::Status::internal("") on local send failure,
/// or mrpc::Status::data_loss("") on local receive failure.
//
// NOTE(cjr): do not annotate this structure with any repr, use repr(Rust)
// and static assert.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TransportStatus {
    Success,
    // The underlying transport error code.
    Error(NonZeroU32),
}

impl TransportStatus {
    #[inline]
    pub fn code(self) -> u32 {
        match self {
            Self::Success => 0,
            Self::Error(code) => code.get(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageMeta {
    pub conn_id: Handle,
    pub service_id: u32,
    pub func_id: u32,
    pub call_id: u32,
    pub len: u64,
    pub msg_type: RpcMsgType,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MessageErased {
    pub meta: MessageMeta,
    pub shm_addr_app: usize,
    pub shm_addr_backend: usize,
}

mod sa {
    use super::*;
    use static_assertions::const_assert_eq;
    use std::mem::size_of;

    const_assert_eq!(size_of::<TransportStatus>(), 4);
    const_assert_eq!(size_of::<RpcId>(), 8);
    const_assert_eq!(size_of::<MessageMeta>(), 32);
    const_assert_eq!(size_of::<MessageErased>(), 48);
}
