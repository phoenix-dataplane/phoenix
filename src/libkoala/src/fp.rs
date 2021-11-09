//! Fast path operations.
use ipc::dp::{Request, ResponseKind};

use crate::{slice_to_range, Error, KL_CTX};

use crate::cm::CmId;
use crate::verbs;
use crate::verbs::{CompletionQueue, WorkCompletion};

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
        KL_CTX.with(|ctx| {
            ctx.dp_tx.send(req)?;
            rx_recv_impl!(ctx.dp_rx, ResponseKind::PostRecv, { Ok(()) })
        })
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
        KL_CTX.with(|ctx| {
            ctx.dp_tx.send(req)?;
            rx_recv_impl!(ctx.dp_rx, ResponseKind::PostSend, { Ok(()) })
        })
    }

    pub fn get_send_comp(&self) -> Result<verbs::WorkCompletion, Error> {
        let req = Request::GetSendComp(self.handle.0);
        KL_CTX.with(|ctx| {
            ctx.dp_tx.send(req)?;
            rx_recv_impl!(ctx.dp_rx, ResponseKind::GetSendComp, wc, { Ok(wc) })
        })
    }

    pub fn get_recv_comp(&self) -> Result<verbs::WorkCompletion, Error> {
        let req = Request::GetRecvComp(self.handle.0);
        KL_CTX.with(|ctx| {
            ctx.dp_tx.send(req)?;
            rx_recv_impl!(ctx.dp_rx, ResponseKind::GetRecvComp, wc, { Ok(wc) })
        })
    }
}

impl CompletionQueue {
    pub fn poll_cq(&self, wc: &mut [WorkCompletion]) -> Result<(), Error> {
        let req = Request::PollCq(self.inner.clone(), wc.len());
        KL_CTX.with(|ctx| {
            ctx.dp_tx.send(req)?;
            rx_recv_impl!(ctx.dp_rx, ResponseKind::PollCq, wc_ret, {
                wc.clone_from_slice(&wc_ret);
                Ok(())
            })
        })
    }
}
