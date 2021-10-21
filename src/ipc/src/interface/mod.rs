use std::borrow::Borrow;

use interface::*;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
#[derive(Serialize, Deserialize)]
pub struct QpInitAttrOwned {
    // no need to serialize qp_context
    pub send_cq: Option<CompletionQueue>,
    pub recv_cq: Option<CompletionQueue>,
    pub srq: Option<SharedReceiveQueue>,
    pub cap: QpCapability,
    pub qp_type: QpType,
    pub sq_sig_all: bool,
}

impl<'ctx, 'send_cq, 'recv_cq, 'srq> FromBorrow<QpInitAttr<'ctx, 'send_cq, 'recv_cq, 'srq>>
    for QpInitAttrOwned
{
    fn from_borrow<T: Borrow<QpInitAttr<'ctx, 'send_cq, 'recv_cq, 'srq>>>(borrow: &T) -> Self {
        let b = borrow.borrow();
        QpInitAttrOwned {
            send_cq: b.send_cq.map(|x| CompletionQueue(x.0)),
            recv_cq: b.recv_cq.map(|x| CompletionQueue(x.0)),
            srq: b.srq.map(|x| SharedReceiveQueue(x.0)),
            cap: b.cap.clone(),
            qp_type: b.qp_type,
            sq_sig_all: b.sq_sig_all,
        }
    }
}

// Get an owned structure from a borrow
pub trait FromBorrow<Borrowed> {
    fn from_borrow<T: Borrow<Borrowed>>(borrow: &T) -> Self;
}
