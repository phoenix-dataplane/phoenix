use std::{io, ops::Range};

use dns_lookup::AddrInfoIter;

use interface::*;
use ipc::interface::*;
use ipc::{cmd, dp};

use crate::{Context, Error};

macro_rules! rx_recv_impl {
    ($rx:expr, $resp:path, $inst:ident, $ok_block:block) => {
        match $rx.recv().map_err(|e| Error::IpcRecvError(e))? {
            $resp(Ok($inst)) => $ok_block,
            $resp(Err(e)) => Err(e.into()),
            _ => {
                panic!("");
            }
        }
    };
}

fn range_ptr_to_u64<T>(range: &Range<*const T>) -> Range<u64> {
    Range {
        start: range.start as u64,
        end: range.end as u64,
    }
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
    ctx.cmd_tx.send(req)?;
    rx_recv_impl!(ctx.cmd_rx, cmd::Response::CreateEp, handle, {
        Ok(CmId(handle))
    })
}

pub fn koala_reg_msgs<T>(
    ctx: &Context,
    id: &CmId,
    range: &Range<*const T>,
) -> Result<MemoryRegion, Error> {
    let req = cmd::Request::RegMsgs(id.0, range_ptr_to_u64(range));
    ctx.cmd_tx.send(req)?;
    rx_recv_impl!(ctx.cmd_rx, cmd::Response::RegMsgs, handle, {
        Ok(MemoryRegion(handle))
    })
}

pub fn koala_post_recv<T>(
    ctx: &Context,
    id: &CmId,
    context: u64,
    range: &Range<*const T>,
    mr: &MemoryRegion,
) -> Result<(), Error> {
    let req = dp::Request::PostRecv(id.0, context, range_ptr_to_u64(range), mr.0);
    ctx.dp_tx.send(req)?;
    rx_recv_impl!(ctx.dp_rx, dp::Response::PostRecv, x, { Ok(x) })
}

pub fn koala_post_send<T>(
    ctx: &Context,
    id: &CmId,
    context: u64,
    range: &Range<*const T>,
    mr: &MemoryRegion,
    flags: i32,
) -> Result<(), Error> {
    let req = dp::Request::PostSend(id.0, context, range_ptr_to_u64(range), mr.0, flags);
    ctx.dp_tx.send(req)?;
    rx_recv_impl!(ctx.dp_rx, dp::Response::PostSend, x, { Ok(x) })
}

pub fn koala_listen(ctx: &Context, id: &CmId, backlog: i32) -> Result<(), Error> {
    let req = cmd::Request::Listen(id.0, backlog);
    ctx.cmd_tx.send(req)?;
    rx_recv_impl!(ctx.cmd_rx, cmd::Response::Listen, x, { Ok(x) })
}

pub fn koala_get_requst(ctx: &Context, listen: &CmId) -> Result<CmId, Error> {
    let req = dp::Request::GetRequest(listen.0);
    ctx.dp_tx.send(req)?;
    rx_recv_impl!(ctx.dp_rx, dp::Response::GetRequest, handle, {
        Ok(CmId(handle))
    })
}

pub fn koala_accept(ctx: &Context, id: &CmId, conn_param: Option<&ConnParam>) -> Result<(), Error> {
    let req = dp::Request::Accept(
        id.0,
        conn_param.map(|param| ConnParamOwned::from_borrow(param)),
    );
    ctx.dp_tx.send(req)?;
    rx_recv_impl!(ctx.dp_rx, dp::Response::Accept, x, { Ok(x) })
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
    ctx.dp_tx.send(req)?;
    rx_recv_impl!(ctx.dp_rx, dp::Response::Connect, x, { Ok(x) })
}

pub fn koala_get_send_comp(ctx: &Context, id: &CmId) -> Result<WorkCompletion, Error> {
    let req = dp::Request::GetSendComp(id.0);
    ctx.dp_tx.send(req)?;
    rx_recv_impl!(ctx.dp_rx, dp::Response::GetSendComp, wc, { Ok(wc) })
}

pub fn koala_get_recv_comp(ctx: &Context, id: &CmId) -> Result<WorkCompletion, Error> {
    let req = dp::Request::GetRecvComp(id.0);
    ctx.dp_tx.send(req)?;
    rx_recv_impl!(ctx.dp_rx, dp::Response::GetRecvComp, wc, { Ok(wc) })
}
