//! RPC data structures
use std::num::NonZeroU32;
use std::ops::BitOr;

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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, Eq, PartialEq, Hash)]
pub struct ImmFlags(pub u8);

/// On sender's side, ImmFlags will be carried by RpcId.
/// On receiver's side, ImmFlags will be carried by imm_data.
impl ImmFlags {
    /// This packet indicates the end of current RPC requests. Always set if FUSE_RPC is 1.
    pub const RPC_ENDING: ImmFlags = ImmFlags(1 << 0);
    /// This packet consists of multiple original packets.
    pub const FUSE_PKT: ImmFlags = ImmFlags(1 << 1);
    /// This packet consists of packets from multiple RPC requests.
    /// When FUSE_RPC is set, RPC_ENDING should have no effect.
    pub const FUSE_RPC: ImmFlags = ImmFlags(1 << 2);

    #[inline(always)]
    pub fn set(&mut self, flag: ImmFlags) {
        self.0 |= flag.0
    }

    // all flags should be set
    #[inline(always)]
    pub fn has_all(&self, flag: ImmFlags) -> bool {
        self.0 & flag.0 == flag.0
    }

    // any flag should be set
    #[inline(always)]
    pub fn has_any(&self, flag: ImmFlags) -> bool {
        self.0 & flag.0 != 0
    }
}

impl BitOr for ImmFlags {
    type Output = ImmFlags;

    #[inline(always)]
    fn bitor(self, rhs: Self) -> Self::Output {
        ImmFlags(self.0 | rhs.0)
    }
}

/// We pack `conn_id` and `call_id` into `RpcId`. It is used it uniquely identify an RPC call.
/// We will take **only lower 24 bits for conn_id**.
/// Should not be used without encoding
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Eq, Hash)]
pub struct RpcId {
    pub conn_id: Handle,
    pub call_id: u32,
    pub flag_bits: ImmFlags,
}

impl RpcId {
    #[inline]
    pub fn new(conn_id: Handle, call_id: u32, flag: u8) -> Self {
        RpcId {
            conn_id,
            call_id,
            flag_bits: ImmFlags(flag),
        }
    }

    #[inline]
    pub fn encode_u64(self) -> u64 {
        (self.flag_bits.0 as u64) << 56
            | (self.conn_id.0 as u64 & 0xffffff) << 32
            | self.call_id as u64
    }

    #[inline]
    pub fn encode_u64_without_flags(self) -> u64 {
        (self.conn_id.0 as u64 & 0xffffff) << 32 | self.call_id as u64
    }

    #[inline]
    pub fn decode_u64(val: u64) -> Self {
        Self::new(
            Handle(((val >> 32) & 0xffffff) as u32),
            val as u32,
            (val >> 56) as u8,
        )
    }

    #[inline]
    pub fn decode_u64_without_flags(val: u64) -> Self {
        Self::new(Handle(((val >> 32) & 0xffffff) as u32), val as u32, 0)
    }

    pub fn clone_without_flags(&self) -> Self {
        Self {
            conn_id: self.conn_id,
            call_id: self.call_id,
            flag_bits: ImmFlags(0),
        }
    }
}

impl PartialEq<Self> for RpcId {
    fn eq(&self, other: &Self) -> bool {
        self.call_id == other.call_id
            && self.flag_bits.0 == other.flag_bits.0
            && (self.conn_id.0 & 0xffffff) == (other.conn_id.0 & 0xffffff)
    }
}

impl From<u64> for RpcId {
    fn from(val: u64) -> Self {
        Self::decode_u64(val)
    }
}

impl From<RpcId> for u64 {
    fn from(val: RpcId) -> Self {
        RpcId::encode_u64(val)
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
    pub call_id: u32,
    /// User associated token. The token will be carried throughout the lifetime of the RPC.
    /// The token can be used to associate RPC reply with its request, or be used as an extra piece
    /// of information passed to the server.
    pub token: u64,
    /// Whether the message is a request or a response.
    pub msg_type: RpcMsgType,
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
    // const_assert_eq!(size_of::<RpcId>(), 8);
    const_assert_eq!(size_of::<MessageMeta>(), 32);
    const_assert_eq!(size_of::<MessageErased>(), 48);
}
