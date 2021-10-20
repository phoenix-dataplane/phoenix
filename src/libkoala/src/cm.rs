use std::io;

use dns_lookup::AddrInfoIter;

use interface::{CmId, ProtectionDomain, QpInitAttr};
use ipc::cmd::{Request, Response};
use ipc::interface::{FromBorrow, QpInitAttrOwned};

use crate::{Context, Error};

/// Creates an identifier that is used to track communication information.
pub fn koala_create_ep(
    ctx: &Context,
    ai: AddrInfoIter,
    pd: Option<&ProtectionDomain>,
    qp_init_attr: Option<&QpInitAttr>,
) -> Result<CmId, Error> {
    let ai_vec = ai
        .map(|a| a.map(interface::AddrInfo::from))
        .collect::<io::Result<Vec<_>>>()?;
    let req = Request::CreateEp(
        ai_vec,
        pd.map(|pd| pd.0),
        qp_init_attr.map(|attr| QpInitAttrOwned::from_borrow(attr)),
    );
    ctx.tx.send(req)?;
    match ctx.rx.recv().map_err(|e| Error::IpcRecvError(e))? {
        Response::CreateEp(Ok(handle)) => Ok(CmId(handle)),
        Response::CreateEp(Err(e)) => Err(e.into()),
        _ => {
            panic!("");
        }
    }
}
