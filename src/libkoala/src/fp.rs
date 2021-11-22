//! Fast path operations.
use std::mem;
use std::sync::atomic::Ordering;

use log::warn;

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
                ctx.dp_wq
                    .borrow_mut()
                    .sender_mut()
                    .send(|ptr, count| unsafe {
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
                ctx.dp_wq
                    .borrow_mut()
                    .sender_mut()
                    .send(|ptr, count| unsafe {
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
        let mut wc = Vec::with_capacity(1);
        let cq = &self.qp.as_ref().unwrap().send_cq;
        loop {
            cq.poll_cq(&mut wc)?;
            if wc.len() == 1 {
                break;
            }
        }
        Ok(wc[0])
    }

    pub fn get_recv_comp(&self) -> Result<verbs::WorkCompletion, Error> {
        let mut wc = Vec::with_capacity(1);
        let cq = &self.qp.as_ref().unwrap().recv_cq;
        loop {
            cq.poll_cq(&mut wc)?;
            if wc.len() == 1 {
                break;
            }
        }
        Ok(wc[0])
    }
}

impl CompletionQueue {
    pub fn poll_cq(&self, wc: &mut Vec<WorkCompletion>) -> Result<(), Error> {
        // poll local buffer first
        unsafe { wc.set_len(0) };
        if !self.buffer.shared.queue.borrow().is_empty() {
            let count = wc.capacity().min(self.buffer.shared.queue.borrow().len());
            // eprintln!("before: buffer_len: {}, count: {}", self.buffer.queue.borrow().len(), count);
            for c in self.buffer.shared.queue.borrow_mut().drain(..count) {
                wc.push(c);
            }
            // eprintln!("after: buffer_len: {}, count: {}", self.buffer.queue.borrow().len(), count);
            return Ok(());
        }
        // if local buffer is empty,
        KL_CTX.with(|ctx| {
            if !self.buffer.shared.outstanding.load(Ordering::Acquire) {
                // 1. Send a poll_cq command to the koala server. This poll_cq command does not have
                // to be sent successfully. Because the user would keep retrying until they get what
                // they expect.
                let req = WorkRequest::PollCq(self.inner);
                ctx.dp_wq
                    .borrow_mut()
                    .sender_mut()
                    .send(|ptr, _count| unsafe {
                        ptr.write(mem::transmute::<WorkRequest, WorkRequestSlot>(req));
                        self.buffer
                            .shared
                            .outstanding
                            .store(true, Ordering::Release);
                        1
                    })?;
            }
            // 2. Poll the shared memory queue, and put into the local buffer. Then return
            // immediately.
            ctx.dp_cq
                .borrow_mut()
                .receiver_mut()
                .recv(|ptr, count| unsafe {
                    // iterate and dispatch
                    for i in 0..count {
                        let c = ptr.add(i).cast::<Completion>().read();
                        if let Some(buffer) = ctx.cq_buffers.borrow().get(&c.cq_handle) {
                            buffer.shared.outstanding.store(false, Ordering::Release);
                            // this is just a notification that outstanding flag should be flapped
                            if c.wc.status != interface::WcStatus::AGAIN {
                                buffer.shared.queue.borrow_mut().push_back(c.wc);
                            }
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
