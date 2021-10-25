use std::io;

use dns_lookup::AddrInfoIter;

use interface::*;
use ipc::cmd::{Request, Response};
use ipc::interface::*;

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

pub fn koala_reg_msgs(ctx: &Context, id: &CmId, addr: u64, len: u64) -> Result<IbvMr, Error> {
    let req = Request::RegMsgs(id.0, addr, len);
    ctx.tx.send(req)?;
    match ctx.rx.recv().map_err(|e| Error::IpcRecvError(e))? {
        Response::RegMsgs(Ok(handle)) => Ok(IbvMr(handle)),
        Response::RegMsgs(Err(e)) => Err(e.into()),
        _ => {
            panic!("");
        }
    }
}

pub fn koala_post_recv(
    ctx: &Context,
    id: &CmId,
    context: u64,
    addr: u64,
    len: u64,
    mr: IbvMr,
) -> Result<(), Error> {
    let req = Request::PostRecv(id.0, context, addr, len, mr);
    ctx.tx.send(req)?;
    match ctx.rx.recv().map_err(|e| Error::IpcRecvError(e))? {
        Response::PostRecv(Ok(())) => Ok(()),
        Response::PostRecv(Err(e)) => Err(e.into()),
        _ => {
            panic!("");
        }
    }
}

pub fn koala_post_send(
    ctx: &Context,
    id: &CmId,
    context: u64,
    addr: u64,
    len: u64,
    mr: IbvMr,
    flags: i32,
) -> Result<(), Error> {
    let req = Request::PostSend(id.0, context, addr, len, mr, flags);
    ctx.tx.send(req)?;
    match ctx.rx.recv().map_err(|e| Error::IpcRecvError(e))? {
        Response::PostSend(Ok(())) => Ok(()),
        Response::PostSend(Err(e)) => Err(e.into()),
        _ => {
            panic!("");
        }
    }
}

pub fn koala_connect(
    ctx: &Context,
    id: &CmId,
    conn_param: Option<&ConnParam>,
) -> Result<(), Error> {
    let req = Request::Connect(
        id.0,
        conn_param.map(|param| ConnParamOwned::from_borrow(param)),
    );
    ctx.tx.send(req)?;
    match ctx.rx.recv().map_err(|e| Error::IpcRecvError(e))? {
        Response::Connect(Ok(())) => Ok(()),
        Response::Connect(Err(e)) => Err(e.into()),
        _ => {
            panic!("");
        }
    }
}
