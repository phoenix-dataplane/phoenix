use std::io;

use dns_lookup::AddrInfoIter;

use interface::*;
use ipc::interface::*;
use ipc::{cmd, dp};

use crate::{Context, Error};

macro_rules! rx_recv_impl {
    ($ctx:expr, $resp:path, $inst:ident, $ok_block:block) => {
        match $ctx.rx.recv().map_err(|e| Error::IpcRecvError(e))? {
            $resp(Ok($inst)) => $ok_block,
            $resp(Err(e)) => Err(e.into()),
            _ => {
                panic!("");
            }
        }
    };
}

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
    let req = cmd::Request::CreateEp(
        ai_vec,
        pd.map(|pd| pd.0),
        qp_init_attr.map(|attr| QpInitAttrOwned::from_borrow(attr)),
    );
    ctx.tx.send(req)?;
    rx_recv_impl!(ctx, cmd::Response::CreateEp, handle, { Ok(CmId(handle)) })
}

pub fn koala_reg_msgs(
    ctx: &Context,
    id: &CmId,
    addr: u64,
    len: u64,
) -> Result<MemoryRegion, Error> {
    let req = cmd::Request::RegMsgs(id.0, addr, len);
    ctx.tx.send(req)?;
    rx_recv_impl!(ctx, cmd::Response::RegMsgs, handle, { Ok(MemoryRegion(handle)) })
}

pub fn koala_post_recv(
    ctx: &Context,
    id: &CmId,
    context: u64,
    addr: u64,
    len: u64,
    mr: MemoryRegion,
) -> Result<(), Error> {
    let req = dp::Request::PostRecv(id.0, context, addr, len, mr);
    ctx.tx.send(req)?;
    rx_recv_impl!(ctx, dp::Response::PostRecv, x, { Ok(x) })
}

pub fn koala_post_send(
    ctx: &Context,
    id: &CmId,
    context: u64,
    addr: u64,
    len: u64,
    mr: MemoryRegion,
    flags: i32,
) -> Result<(), Error> {
    let req = dp::Request::PostSend(id.0, context, addr, len, mr, flags);
    ctx.tx.send(req)?;
    rx_recv_impl!(ctx, dp::Response::PostSend, x, { Ok(x) })
}

pub fn koala_connect(
    ctx: &Context,
    id: &CmId,
    conn_param: Option<&ConnParam>,
) -> Result<(), Error> {
    let req = dp::Request::Connect(
        id.0,
        conn_param.map(|param| ConnParamOwned::from_borrow(param)),
    );
    ctx.tx.send(req)?;
    rx_recv_impl!(ctx, dp::Response::Connect, x, { Ok(x) })
}

pub fn koala_get_send_comp(ctx: &Context, id: &CmId) -> Result<WorkCompletion, Error> {
    let req = dp::Request::GetSendComp(id.0);
    ctx.tx.send(req)?;
    rx_recv_impl!(ctx, dp::Response::GetSendComp, wc, { Ok(wc) })
}

pub fn koala_get_recv_comp(ctx: &Context, id: &CmId) -> Result<WorkCompletion, Error> {
    let req = dp::Request::GetRecvComp(id.0);
    ctx.tx.send(req)?;
    rx_recv_impl!(ctx, dp::Response::GetRecvComp, wc, { Ok(wc) })
}
