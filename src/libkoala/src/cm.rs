use interface::{
    addrinfo::{AddrInfo, AddrInfoHints},
    CmId, ProtectionDomain, QpInitAttr,
    MemoryRegion, ConnParam,
};
use ipc::cmd::{Request, Response};
use ipc::interface::{FromBorrow, QpInitAttrOwned, ConnParamOwned};

use crate::{Context, Error, slice_to_range};

/// Creates an identifier that is used to track communication information.
pub fn create_ep(
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
    ctx.cmd_tx.send(req)?;
    match ctx.cmd_rx.recv().map_err(|e| Error::IpcRecvError(e))? {
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
    ctx.cmd_tx.send(req)?;
    match ctx.cmd_rx.recv().map_err(|e| Error::IpcRecvError(e))? {
        Response::GetAddrInfo(Ok(ai)) => Ok(ai),
        Response::GetAddrInfo(Err(e)) => Err(e.into()),
        _ => panic!(""),
    }
}

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

pub fn reg_msgs<T>(
    ctx: &Context,
    id: &CmId,
    buffer: &[T],
) -> Result<MemoryRegion, Error> {
    let req = Request::RegMsgs(id.0, slice_to_range(buffer));
    ctx.cmd_tx.send(req)?;
    rx_recv_impl!(ctx.cmd_rx, Response::RegMsgs, handle, {
        Ok(MemoryRegion(handle))
    })
}

pub fn listen(ctx: &Context, id: &CmId, backlog: i32) -> Result<(), Error> {
    let req = Request::Listen(id.0, backlog);
    ctx.cmd_tx.send(req)?;
    rx_recv_impl!(ctx.cmd_rx, Response::Listen, x, { Ok(x) })
}

pub fn get_requst(ctx: &Context, listen: &CmId) -> Result<CmId, Error> {
    let req = Request::GetRequest(listen.0);
    ctx.cmd_tx.send(req)?;
    rx_recv_impl!(ctx.cmd_rx, Response::GetRequest, handle, {
        Ok(CmId(handle))
    })
}

pub fn accept(ctx: &Context, id: &CmId, conn_param: Option<&ConnParam>) -> Result<(), Error> {
    let req = Request::Accept(
        id.0,
        conn_param.map(|param| ConnParamOwned::from_borrow(param)),
    );
    ctx.cmd_tx.send(req)?;
    rx_recv_impl!(ctx.cmd_rx, Response::Accept, x, { Ok(x) })
}

pub fn connect(ctx: &Context, id: &CmId, conn_param: Option<&ConnParam>) -> Result<(), Error> {
    let req = Request::Connect(
        id.0,
        conn_param.map(|param| ConnParamOwned::from_borrow(param)),
    );
    ctx.cmd_tx.send(req)?;
    rx_recv_impl!(ctx.cmd_rx, Response::Connect, x, { Ok(x) })
}
