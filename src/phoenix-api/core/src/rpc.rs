//! RPC data structures
#![cfg(feature = "mrpc")]
use std::fmt;
use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::Handle;

#[repr(C)]
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize,
)]
pub struct Token(pub usize);

// User must explicitly construct Token from usize.

impl From<Token> for usize {
    fn from(val: Token) -> usize {
        val.0
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RpcMsgType {
    Request,
    Response,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct CallId(pub u64);

impl From<usize> for CallId {
    fn from(val: usize) -> Self {
        CallId(val as u64)
    }
}

impl fmt::Display for CallId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CallId({})", self.0)
    }
}

/// We pack `conn_id` and `call_id` into `RpcId`. It is used it uniquely identify an RPC call.
#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RpcId(pub Handle, pub CallId);

impl RpcId {
    #[inline]
    pub fn new(conn_id: Handle, call_id: CallId) -> Self {
        RpcId(conn_id, call_id)
    }
}

/// Transport layer status.
///
/// This transport error will be translated to mrpc::Status::internal("") on local send failure,
/// or mrpc::Status::data_loss("") on local receive failure.
//
// NOTE(cjr): Do not annotate this structure with any repr. Use repr(Rust)
// and static assertions.
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
    /// Connection handle.
    pub conn_id: Handle,
    /// Service identifier.
    pub service_id: u32,
    /// Function identifier. A hash of full-qualified path.
    pub func_id: u32,
    /// Identifier for each unique RPC invocation.
    pub call_id: CallId,
    /// User associated token. The token will be carried throughout the lifetime of the RPC.
    /// The token can be used to associate RPC reply with its request, or be used as an extra piece
    /// of information passed to the server.
    pub token: u64,
    /// Whether the message is a request or a response.
    pub msg_type: RpcMsgType,
    // Request credits and timestamp as used by the BreakWater policy.
    pub request_credits: u16,
    pub request_timestamp: u16,

}

#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MessageErased {
    pub meta: MessageMeta,
    // It is fine to use usize here because these two addresses are machine-local.
    pub shm_addr_app: usize,
    pub shm_addr_backend: usize,
}

mod sa {
    use super::*;
    use static_assertions::const_assert_eq;
    use std::mem::size_of;

    const_assert_eq!(size_of::<Token>(), size_of::<usize>());
    const_assert_eq!(size_of::<TransportStatus>(), 4);
    const_assert_eq!(size_of::<RpcId>(), 16);
    const_assert_eq!(size_of::<MessageMeta>(), 40);
    const_assert_eq!(size_of::<MessageErased>(), 56);
}
