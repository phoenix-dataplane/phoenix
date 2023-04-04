//! RPC data structures.
#![cfg(feature = "mrpc")]
use std::fmt;
use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::Handle;

/// Associates custom user data with an RPC.
///
/// `Token` is a wrapper around a `usize`.
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

/// Indicates the direction of an RPC message.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RpcMsgType {
    Request,
    Response,
}

/// An `u64` associated with an RPC.
///
/// This ID is guaranteed to be unique for RPC calls within a connection.
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

/// A wrapper around a [`Handle`] and a [`CallId`] to uniquely identify an RPC for an application.
///
/// The `Handle` inside identifies the connection ID for this RPC.
#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RpcId(pub Handle, pub CallId);

impl RpcId {
    /// Constructs the RPC from `conn_id` and `call_id`.
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
    /// Converting a [`TransportStatus`] to a `u32`.
    ///
    /// Returns 0 for Success. Returns the underlying error code otherwise.
    #[inline]
    pub fn code(self) -> u32 {
        match self {
            Self::Success => 0,
            Self::Error(code) => code.get(),
        }
    }
}

/// The metadata prepended to each RPC message.
#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum StatusCode {
    Success = 0,
    AccessDenied = 1,
    Unknown = 2,
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
    /// Plugin specific status code.
    pub status_code: StatusCode,
}

/// An RPC descriptor.
///
/// Contains the metadata of the RPC message and a group of pointer that points to the location of
/// the RPC argument/response on the shared memory heap.
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

    const_assert_eq!(size_of::<StatusCode>(), 4);
    const_assert_eq!(size_of::<Token>(), size_of::<usize>());
    const_assert_eq!(size_of::<TransportStatus>(), 4);
    const_assert_eq!(size_of::<RpcId>(), 16);
    const_assert_eq!(size_of::<MessageMeta>(), 40);
    const_assert_eq!(size_of::<MessageErased>(), 56);
}
