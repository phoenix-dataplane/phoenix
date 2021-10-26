use serde::{Deserialize, Serialize};
use std::{any::Any, vec};
use thiserror::Error;

pub mod addrinfo;
pub use addrinfo::AddrInfo;

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum Error {
    #[error("rdmacm internal error: {0}")]
    RdmaCm(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Handle(pub usize);

#[derive(Debug, Serialize, Deserialize)]
pub struct CmId(pub Handle);

#[derive(Debug, Serialize, Deserialize)]
pub struct CompletionQueue(pub Handle);

#[derive(Debug, Serialize, Deserialize)]
pub struct ProtectionDomain(pub Handle);

#[derive(Debug, Serialize, Deserialize)]
pub struct SharedReceiveQueue(pub Handle);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QpType {
    RC,
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

pub struct QpInitAttr<'ctx, 'send_cq, 'recv_cq, 'srq> {
    pub qp_context: Option<&'ctx dyn Any>,
    pub send_cq: Option<&'send_cq CompletionQueue>,
    pub recv_cq: Option<&'recv_cq CompletionQueue>,
    pub srq: Option<&'srq SharedReceiveQueue>,
    pub cap: QpCapability,
    pub qp_type: QpType,
    pub sq_sig_all: bool,
}

#[derive(Serialize, Deserialize)]
pub struct MemoryRegion(pub Handle);

pub struct ConnParam<'priv_data> {
    pub private_data: Option<&'priv_data [u8]>,
    pub responder_resources: u8,
    pub initiator_depth: u8,
    pub flow_control: u8,
    pub retry_count: u8,
    pub rnr_retry_count: u8,
    pub srq: u8,
    pub qp_num: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum WCStatus {
    WC_SUCCESS,
    WC_LOC_LEN_ERR,
    WC_LOC_QP_OP_ERR,
    WC_LOC_EEC_OP_ERR,
    WC_LOC_PROT_ERR,
    WC_WR_FLUSH_ERR,
    WC_MW_BIND_ERR,
    WC_BAD_RESP_ERR,
    WC_LOC_ACCESS_ERR,
    WC_REM_INV_REQ_ERR,
    WC_REM_ACCESS_ERR,
    WC_REM_OP_ERR,
    WC_RETRY_EXC_ERR,
    WC_RNR_RETRY_EXC_ERR,
    WC_LOC_RDD_VIOL_ERR,
    WC_REM_INV_RD_REQ_ERR,
    WC_REM_ABORT_ERR,
    WC_INV_EECN_ERR,
    WC_INV_EEC_STATE_ERR,
    WC_FATAL_ERR,
    WC_RESP_TIMEOUT_ERR,
    WC_GENERAL_ERR,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum WCOpcode {
    WC_SEND,
    WC_RDMA_WRITE,
    WC_RDMA_READ,
    WC_COMP_SWAP,
    WC_FETCH_ADD,
    WC_BIND_MW,
    WC_LOCAL_INV,
    WC_TSO,
    WC_RECV,
    WC_RECV_RDMA_WITH_IMM,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkCompletion {
    pub wr_id: u64,
    pub status: WCStatus,
    pub opcode: WCOpcode,
    pub vendor_err: i32,
    pub byte_len: i32,
}
