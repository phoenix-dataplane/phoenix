use std::any::Any;
use std::borrow::Borrow;
use std::fs::File;

use interface::returned;
use interface::Handle;

use crate::FromBorrow;

// Re-exports
pub use interface::{QpCapability, QpType};
pub use interface::{SendFlags, WcFlags, WcOpcode, WcStatus, WorkCompletion};

pub struct ProtectionDomain {
    pub(crate) inner: interface::ProtectionDomain,
}

pub struct CompletionQueue {
    pub(crate) inner: interface::CompletionQueue,
}

impl From<returned::CompletionQueue> for CompletionQueue {
    fn from(other: returned::CompletionQueue) -> Self {
        CompletionQueue {
            inner: other.handle,
        }
    }
}

pub struct SharedReceiveQueue {
    pub(crate) inner: interface::SharedReceiveQueue,
}

#[derive(Debug)]
pub struct MemoryRegion {
    pub(crate) inner: interface::MemoryRegion,
    memfd: File,
}

impl MemoryRegion {
    pub fn new(handle: Handle, memfd: File) -> Self {
        MemoryRegion {
            inner: interface::MemoryRegion(handle),
            memfd,
        }
    }
}

pub struct QueuePair {
    inner: interface::QueuePair,
    pub send_cq: CompletionQueue,
    pub recv_cq: CompletionQueue,
}

impl From<returned::QueuePair> for QueuePair {
    fn from(other: returned::QueuePair) -> Self {
        QueuePair {
            inner: other.handle,
            send_cq: CompletionQueue::from(other.send_cq),
            recv_cq: CompletionQueue::from(other.recv_cq),
        }
    }
}

pub struct QpInitAttr<'ctx> {
    pub qp_context: Option<&'ctx dyn Any>,
    pub send_cq: Option<CompletionQueue>,
    pub recv_cq: Option<CompletionQueue>,
    pub srq: Option<SharedReceiveQueue>,
    pub cap: QpCapability,
    pub qp_type: QpType,
    pub sq_sig_all: bool,
}

impl<'ctx> FromBorrow<QpInitAttr<'ctx>> for interface::QpInitAttr {
    fn from_borrow<T: Borrow<QpInitAttr<'ctx>>>(borrow: &T) -> Self {
        let b = borrow.borrow();
        interface::QpInitAttr {
            send_cq: b.send_cq.as_ref().map(|x| x.inner.clone()),
            recv_cq: b.recv_cq.as_ref().map(|x| x.inner.clone()),
            srq: b.srq.as_ref().map(|x| x.inner.clone()),
            cap: b.cap.clone(),
            qp_type: b.qp_type,
            sq_sig_all: b.sq_sig_all,
        }
    }
}

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

impl<'priv_data> FromBorrow<ConnParam<'priv_data>> for interface::ConnParam {
    fn from_borrow<T: Borrow<ConnParam<'priv_data>>>(borrow: &T) -> Self {
        let b = borrow.borrow();
        interface::ConnParam {
            private_data: b.private_data.map(|x| x.to_owned()),
            responder_resources: b.responder_resources,
            initiator_depth: b.initiator_depth,
            flow_control: b.flow_control,
            retry_count: b.retry_count,
            rnr_retry_count: b.rnr_retry_count,
            srq: b.srq,
            qp_num: b.qp_num,
        }
    }
}
