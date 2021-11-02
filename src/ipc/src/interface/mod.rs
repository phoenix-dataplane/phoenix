use std::borrow::Borrow;

use interface::*;
use serde::{Deserialize, Serialize};

// Get an owned structure from a borrow
pub trait FromBorrow<Borrowed> {
    fn from_borrow<T: Borrow<Borrowed>>(borrow: &T) -> Self;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QpInitAttrOwned {
    // no need to serialize qp_context
    pub send_cq: Option<CompletionQueue>,
    pub recv_cq: Option<CompletionQueue>,
    pub srq: Option<SharedReceiveQueue>,
    pub cap: QpCapability,
    pub qp_type: QpType,
    pub sq_sig_all: bool,
}

impl<'ctx, 'scq, 'rcq, 'srq> FromBorrow<QpInitAttr<'ctx, 'scq, 'rcq, 'srq>> for QpInitAttrOwned {
    fn from_borrow<T: Borrow<QpInitAttr<'ctx, 'scq, 'rcq, 'srq>>>(borrow: &T) -> Self {
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

#[derive(Debug, Serialize, Deserialize)]
pub struct ConnParamOwned {
    private_data: Option<Vec<u8>>,
    responder_resources: u8,
    initiator_depth: u8,
    flow_control: u8,
    retry_count: u8,
    rnr_retry_count: u8,
    srq: u8,
    qp_num: u32,
}

impl<'priv_data> FromBorrow<ConnParam<'priv_data>> for ConnParamOwned {
    fn from_borrow<T: Borrow<ConnParam<'priv_data>>>(borrow: &T) -> Self {
        let b = borrow.borrow();
        ConnParamOwned {
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
