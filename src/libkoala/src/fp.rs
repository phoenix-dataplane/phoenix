//! Fast path operations.
use std::mem;

use ipc::buf;
use ipc::dp::{Completion, WorkRequest, WorkRequestSlot};

use crate::{Error, KL_CTX};

use crate::cm::CmId;
use crate::verbs;
use crate::verbs::{CompletionQueue, WorkCompletion};

impl CmId {
    pub unsafe fn post_recv<T>(
        &self,
        context: u64,
        buffer: &mut [T],
        mr: &verbs::MemoryRegion,
    ) -> Result<(), Error> {
        let req = WorkRequest::PostRecv(
            self.handle.0,
            context,
            buf::Buffer::from(buffer),
            mr.inner.0,
        );
        KL_CTX.with(|ctx| {
            // This WR must be successfully sent.
            let mut sent = false;
            while !sent {
                ctx.dp_wq.borrow_mut().send_raw(|ptr, count| unsafe {
                    debug_assert!(count >= 1);
                    ptr.cast::<WorkRequest>().write(req);
                    sent = true;
                    1
                })?;
            }
            Ok(())
        })
    }

    pub fn post_send<T>(
        &self,
        context: u64,
        buffer: &[T],
        mr: &verbs::MemoryRegion,
        flags: verbs::SendFlags,
    ) -> Result<(), Error> {
        let req = WorkRequest::PostSend(
            self.handle.0,
            context,
            buf::Buffer::from(buffer),
            mr.inner.0,
            flags,
        );
        KL_CTX.with(|ctx| {
            let mut sent = false;
            while !sent {
                ctx.dp_wq.borrow_mut().send_raw(|ptr, count| unsafe {
                    debug_assert!(count >= 1);
                    ptr.cast::<WorkRequest>().write(req);
                    sent = true;
                    1
                })?;
            }
            Ok(())
        })
    }

    pub fn get_send_comp(&self) -> Result<verbs::WorkCompletion, Error> {
        unimplemented!();
        // let req = Request::GetSendComp(self.handle.0);
        // KL_CTX.with(|ctx| {
        //     ctx.dp_tx.send(req)?;
        //     rx_recv_impl!(ctx.dp_rx, ResponseKind::GetSendComp, wc, { Ok(wc) })
        // })
    }

    pub fn get_recv_comp(&self) -> Result<verbs::WorkCompletion, Error> {
        unimplemented!();
        // let req = Request::GetRecvComp(self.handle.0);
        // KL_CTX.with(|ctx| {
        //     ctx.dp_tx.send(req)?;
        //     rx_recv_impl!(ctx.dp_rx, ResponseKind::GetRecvComp, wc, { Ok(wc) })
        // })
    }
}

impl CompletionQueue {
    pub fn poll_cq(&self, wc: &mut Vec<WorkCompletion>) -> Result<(), Error> {
        // poll local buffer first
        if !self.buffer.queue.borrow().is_empty() {
            let count = wc.capacity().min(self.buffer.queue.borrow().len());
            unsafe { wc.set_len(0) };
            for c in self.buffer.queue.borrow_mut().drain(..count) {
                wc.push(c);
            }
            // (&wc[..count]).copy_from_slice(&self.buffer.queue.borrow()[..count]);
            // self.local_buffer.drain(..count);
            return Ok(());
        }
        // if local buffer is empty,
        let req = WorkRequest::PollCq(self.inner);
        KL_CTX.with(|ctx| {
            // 1. send a poll_cq command to the koala server
            // This poll_cq command does not have to be sent successfully. Because
            // the user would keep retrying until they get what they expect.
            ctx.dp_wq.borrow_mut().send_raw(|ptr, _count| unsafe {
                ptr.write(mem::transmute::<WorkRequest, WorkRequestSlot>(req));
                1
            })?;
            // 2. poll the shared memory queue and return immediately
            ctx.dp_cq.borrow_mut().receive_raw(|ptr, count| unsafe {
                // iterate and dispatch
                for i in 0..count {
                    let c = ptr.add(i).cast::<Completion>().read();
                    if let Some(buffer) = ctx.cq_buffers.borrow().get(&c.cq_handle) {
                        buffer.queue.borrow_mut().push_back(c.wc);
                    } else {
                        eprintln!("no corresponding entry for {:?}", c);
                    }
                }
                count
            })?;
            Ok(())
        })
    }
}
