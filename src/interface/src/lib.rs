use std::any::Any;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod addrinfo;
pub use addrinfo::AddrInfo;

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum Error {
    #[error("rdmacm internal error: {0}")]
    RdmaCm(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Serialize, Deserialize)]
pub struct Handle(pub usize);

#[derive(Serialize, Deserialize)]
pub struct CmId(pub Handle);

#[derive(Serialize, Deserialize)]
pub struct CompletionQueue(pub Handle);

#[derive(Serialize, Deserialize)]
pub struct ProtectionDomain(pub Handle);

#[derive(Serialize, Deserialize)]
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
