use interface::{CmId, MemoryRegion, SendFlags, WorkCompletion};
use ipc::dp::{Request, Response};

use crate::{Context, Error, slice_to_range};

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

pub fn post_recv<T>(
    ctx: &Context,
    id: &CmId,
    context: u64,
    buffer: &[T],
    mr: &MemoryRegion,
) -> Result<(), Error> {
    let req = Request::PostRecv(id.0, context, slice_to_range(buffer), mr.handle);
    ctx.dp_tx.send(req)?;
    rx_recv_impl!(ctx.dp_rx, Response::PostRecv, x, { Ok(x) })
}

pub fn post_send<T>(
    ctx: &Context,
    id: &CmId,
    context: u64,
    buffer: &[T],
    mr: &MemoryRegion,
    flags: SendFlags,
) -> Result<(), Error> {
    let req = Request::PostSend(id.0, context, slice_to_range(buffer), mr.handle, flags);
    ctx.dp_tx.send(req)?;
    rx_recv_impl!(ctx.dp_rx, Response::PostSend, x, { Ok(x) })
}

pub fn get_send_comp(ctx: &Context, id: &CmId) -> Result<WorkCompletion, Error> {
    let req = Request::GetSendComp(id.0);
    ctx.dp_tx.send(req)?;
    rx_recv_impl!(ctx.dp_rx, Response::GetSendComp, wc, { Ok(wc) })
}

pub fn get_recv_comp(ctx: &Context, id: &CmId) -> Result<WorkCompletion, Error> {
    let req = Request::GetRecvComp(id.0);
    ctx.dp_tx.send(req)?;
    rx_recv_impl!(ctx.dp_rx, Response::GetRecvComp, wc, { Ok(wc) })
}
