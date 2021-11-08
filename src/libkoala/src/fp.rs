//! Fast path operations.
use ipc::dp::{Request, ResponseKind};

use crate::{slice_to_range, Error};

use crate::verbs;
use crate::cm::CmId;

macro_rules! rx_recv_impl {
    ($rx:expr, $resp:path, $inst:ident, $ok_block:block) => {
        match $rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
            Ok($resp($inst)) => $ok_block,
            Err(e) => Err(e.into()),
            _ => panic!(""),
        }
    };
    ($rx:expr, $resp:path, $ok_block:block) => {
        match $rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
            Ok($resp) => $ok_block,
            Err(e) => Err(e.into()),
            _ => panic!(""),
        }
    };
}

impl CmId {
    pub unsafe fn post_recv<T>(
        &self,
        context: u64,
        buffer: &mut [T],
        mr: &verbs::MemoryRegion,
    ) -> Result<(), Error> {
        let req = Request::PostRecv(self.handle.0, context, slice_to_range(buffer), mr.inner.0);
        self.ctx.dp_tx.send(req)?;
        rx_recv_impl!(self.ctx.dp_rx, ResponseKind::PostRecv, { Ok(()) })
    }

    pub fn post_send<T>(
        &self,
        context: u64,
        buffer: &[T],
        mr: &verbs::MemoryRegion,
        flags: verbs::SendFlags,
    ) -> Result<(), Error> {
        let req = Request::PostSend(
            self.handle.0,
            context,
            slice_to_range(buffer),
            mr.inner.0,
            flags,
        );
        self.ctx.dp_tx.send(req)?;
        rx_recv_impl!(self.ctx.dp_rx, ResponseKind::PostSend, { Ok(()) })
    }

    pub fn get_send_comp(&self) -> Result<verbs::WorkCompletion, Error> {
        let req = Request::GetSendComp(self.handle.0);
        self.ctx.dp_tx.send(req)?;
        rx_recv_impl!(self.ctx.dp_rx, ResponseKind::GetSendComp, wc, { Ok(wc) })
    }

    pub fn get_recv_comp(&self) -> Result<verbs::WorkCompletion, Error> {
        let req = Request::GetRecvComp(self.handle.0);
        self.ctx.dp_tx.send(req)?;
        rx_recv_impl!(self.ctx.dp_rx, ResponseKind::GetRecvComp, wc, { Ok(wc) })
    }
}
