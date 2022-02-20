use std::num::NonZeroU32;
use std::os::unix::io::RawFd;

use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod addrinfo;
pub mod engine;

#[derive(Debug, Clone, Error, Serialize, Deserialize)]
pub enum Error {
    #[error("{0}")]
    Generic(String),
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Handle(pub u32);

impl Handle {
    pub const INVALID: Handle = Handle(u32::MAX);
}

pub trait AsHandle {
    #[must_use]
    fn as_handle(&self) -> Handle;
}

impl AsHandle for RawFd {
    #[inline]
    fn as_handle(&self) -> Handle {
        if *self >= 0 {
            Handle(*self as _)
        } else {
            Handle::INVALID
        }
    }
}

// These data struct can be `Copy` because they are only for IPC use.

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CmId(pub Handle);

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EventChannel(pub Handle);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompletionQueue(pub Handle);

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ProtectionDomain(pub Handle);

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SharedReceiveQueue(pub Handle);

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct QueuePair(pub Handle);

pub mod returned {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct ProtectionDomain {
        pub handle: super::ProtectionDomain,
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct CompletionQueue {
        pub handle: super::CompletionQueue,
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct QueuePair {
        pub handle: super::QueuePair,
        pub pd: ProtectionDomain,
        pub send_cq: CompletionQueue,
        pub recv_cq: CompletionQueue,
    }

    #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
    pub struct CmId {
        pub handle: super::CmId,
        // could be empty for passive CmId (listener).
        pub qp: Option<QueuePair>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct MemoryRegion {
        pub handle: super::MemoryRegion,
        pub rkey: super::RemoteKey,
    }
}

/// The type of QP used for communciation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QpType {
    /// reliable connection
    RC,
    /// unreliable datagram
    UD,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct QpCapability {
    pub max_send_wr: u32,
    pub max_recv_wr: u32,
    pub max_send_sge: u32,
    pub max_recv_sge: u32,
    pub max_inline_data: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QpInitAttr {
    // no need to serialize qp_context
    pub send_cq: Option<CompletionQueue>,
    pub recv_cq: Option<CompletionQueue>,
    pub srq: Option<SharedReceiveQueue>,
    pub cap: QpCapability,
    pub qp_type: QpType,
    pub sq_sig_all: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnParam {
    pub private_data: Option<Vec<u8>>,
    pub responder_resources: u8,
    pub initiator_depth: u8,
    pub flow_control: u8,
    pub retry_count: u8,
    pub rnr_retry_count: u8,
    pub srq: u8,
    pub qp_num: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MemoryRegion(pub Handle);

/// A key that authorizes direct memory access to a memory region.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RemoteKey {
    pub addr: u64,
    pub rkey: u32,
}

// NOTE(cjr): do not annotate this structure with any repr, use repr(Rust)
// and static assert.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WcStatus {
    Success,
    // 1-23, error code from verbs, maybe non-exhaustive
    // 1024, EAGAIN: no entry returned, user should poll again
    Error(NonZeroU32),
}

impl WcStatus {
    pub const AGAIN: WcStatus = WcStatus::Error(unsafe { NonZeroU32::new_unchecked(1024) });
}

#[repr(u32)]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WcOpcode {
    Send = 0,
    RdmaWrite = 1,
    RdmaRead = 2,
    Recv = 128,
    RecvRdmaWithImm = 129,
    Invalid = 255,
}

bitflags! {
    /// Flags of the completed WR.
    #[repr(C)]
    #[derive(Serialize, Deserialize)]
    #[derive(Default)]
    pub struct WcFlags: u32 {
        /// GRH is present (valid only for UD QPs).
        const GRH = 0b00000001;
        /// Immediate data value is valid.
        const WITH_IMM = 0b00000010;
    }

    /// Flags of the WR properties.
    #[repr(C)]
    #[derive(Serialize, Deserialize)]
    #[derive(Default)]
    pub struct SendFlags: u32 {
        /// Set the fence indicator. Valid only for QPs with Transport Service Type RC.
        const FENCE = 0b00000001;
        /// Set the completion notification indicator. Relevant only if QP was created with
        /// sq_sig_all=0.
        const SIGNALED = 0b00000010;
        /// Set the solicited event indicator. Valid only for Send and RDMA Write with immediate.
        const SOLICITED = 0b00000100;
        /// Send data in given gather list as inline data in a send WQE.  Valid only for Send and
        /// RDMA Write.  The L_Key will not be checked.
        const INLINE = 0b00001000;
    }

    /// Access flags of a memory region.
    #[repr(C)]
    #[derive(Serialize, Deserialize)]
    pub struct AccessFlags: u32 {
        /// Enables Local Write Access.
        const LOCAL_WRITE = 0b00000001;
        /// Enables Remote Write Access.
        const REMOTE_WRITE = 0b00000010;
        /// Enables Remote Read Access.
        const REMOTE_READ = 0b00000100;
        /// Enables Remote Atomic Operation Access.
        const REMOTE_ATOMIC = 0b00001000;
    }
}

/// A structure represent completion of some work.
#[repr(C, align(8))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WorkCompletion {
    pub wr_id: u64,
    pub status: WcStatus,
    pub opcode: WcOpcode,
    pub vendor_err: u32,
    pub byte_len: u32,
    pub imm_data: u32,
    pub qp_num: u32,
    pub ud_src_qp: u32,
    pub wc_flags: WcFlags,
}

impl WorkCompletion {
    pub const fn new_vendor_err(wr_id: u64, status: WcStatus, vendor_err: u32) -> Self {
        WorkCompletion {
            wr_id,
            status,
            opcode: WcOpcode::Invalid,
            vendor_err,
            byte_len: 0,
            imm_data: 0,
            qp_num: 0,
            ud_src_qp: 0,
            wc_flags: WcFlags::empty(),
        }
    }

    pub const fn again() -> Self {
        WorkCompletion {
            wr_id: 0,
            status: WcStatus::AGAIN,
            opcode: WcOpcode::Invalid,
            vendor_err: 0,
            byte_len: 0,
            imm_data: 0,
            qp_num: 0,
            ud_src_qp: 0,
            wc_flags: WcFlags::empty(),
        }
    }
}

// TODO(cjr): add static assert to make sure WorkCompletion is compatible with ibv_wc
mod sa {
    use super::*;
    use static_assertions::const_assert_eq;
    use std::mem::size_of;

    const_assert_eq!(size_of::<WcStatus>(), 4);
    const_assert_eq!(size_of::<WcOpcode>(), 4);
    const_assert_eq!(size_of::<WorkCompletion>(), 40);
}
