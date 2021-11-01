use interface::{addrinfo::{AddrInfo, AddrInfoHints}, CmId, ProtectionDomain, QpInitAttr};
use ipc::cmd::{Request, Response};
use ipc::interface::{FromBorrow, QpInitAttrOwned};

use crate::{Context, Error};

/// Creates an identifier that is used to track communication information.
pub fn koala_create_ep(
    ctx: &Context,
    ai: &AddrInfo,
    pd: Option<&ProtectionDomain>,
    qp_init_attr: Option<&QpInitAttr>,
) -> Result<CmId, Error> {
    let req = Request::CreateEp(
        ai.clone(),
        pd.map(|pd| pd.0),
        qp_init_attr.map(|attr| QpInitAttrOwned::from_borrow(attr)),
    );
    ctx.tx.send(req)?;
    match ctx.rx.recv().map_err(|e| Error::IpcRecvError(e))? {
        Response::CreateEp(Ok(handle)) => Ok(CmId(handle)),
        Response::CreateEp(Err(e)) => Err(e.into()),
        _ => panic!(""),
    }
}

/// Address and route resolution service.
pub fn getaddrinfo(
    ctx: &Context,
    node: Option<&str>,
    service: Option<&str>,
    hints: Option<&AddrInfoHints>,
) -> Result<AddrInfo, Error> {
    let req = Request::GetAddrInfo(
        node.map(String::from),
        service.map(String::from),
        hints.map(AddrInfoHints::clone),
    );
    ctx.tx.send(req)?;
    match ctx.rx.recv().map_err(|e| Error::IpcRecvError(e))? {
        Response::GetAddrInfo(Ok(ai)) => Ok(ai),
        Response::GetAddrInfo(Err(e)) => Err(e.into()),
        _ => panic!(""),
    }
}
