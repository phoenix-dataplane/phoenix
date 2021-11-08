use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod addrinfo;

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum Error {
    #[error("{0}")]
    Generic(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Handle(pub usize);

impl From<u32> for Handle {
    fn from(x: u32) -> Self {
        Handle(x as _)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmId(pub Handle);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionQueue(pub Handle);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectionDomain(pub Handle);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedReceiveQueue(pub Handle);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuePair(pub Handle);


pub mod returned {
    use serde::{Serialize, Deserialize};

    #[derive(Debug, Serialize, Deserialize)]
    pub struct CompletionQueue {
        pub handle: super::CompletionQueue,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct QueuePair {
        pub handle: super::QueuePair,
        pub send_cq: CompletionQueue,
        pub recv_cq: CompletionQueue,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct CmId {
        pub handle: super::CmId,
        pub qp: QueuePair,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QpCapability {
    pub max_send_wr: u32,
    pub max_recv_wr: u32,
    pub max_send_sge: u32,
    pub max_recv_sge: u32,
    pub max_inline_data: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QpInitAttr {
    // no need to serialize qp_context
    pub send_cq: Option<CompletionQueue>,
    pub recv_cq: Option<CompletionQueue>,
    pub srq: Option<SharedReceiveQueue>,
    pub cap: QpCapability,
    pub qp_type: QpType,
    pub sq_sig_all: bool,
}

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug)]
pub struct MemoryRegion(pub Handle);

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WcStatus {
    Success,
    Error(u32),
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WcOpcode {
    Send,
    RdmaWrite,
    RdmaRead,
    Recv,
    RecvRdmaWithImm,
    Invalid,
}

bitflags! {
    /// Flags of the completed WR.
    #[derive(Serialize, Deserialize)]
    #[derive(Default)]
    pub struct WcFlags: u32 {
        /// GRH is present (valid only for UD QPs).
        const GRH = 0b00000001;
        /// Immediate data value is valid.
        const WITH_IMM = 0b00000010;
    }

    /// Flags of the WR properties.
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
}

/// A structure represent completion of some work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkCompletion {
    pub wr_id: u64,
    pub status: WcStatus,
    pub opcode: WcOpcode,
    pub vendor_err: u32,
    pub byte_len: u32,
    pub imm_data: u32,
    pub wc_flags: WcFlags,
}
