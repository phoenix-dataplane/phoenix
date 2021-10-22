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
pub struct IbvMr(pub Handle);

struct ConnParam<'priv_data> {
    private_data: &'priv_data [u8],
    responder_resources: u8,
    initiator_depth: u8,
    flow_control: u8,
    retry_count: u8,
    rnr_retry_count: u8,
    srq: u8,
    qp_num: u32,
}
