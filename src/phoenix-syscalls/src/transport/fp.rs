//! Fast path operations.
use std::mem;
use std::slice::SliceIndex;
use std::sync::atomic::Ordering;

use phoenix_api::transport::rdma::dp::{Completion, WorkRequest, WorkRequestSlot};
use phoenix_api::{buf, net};

use crate::transport::{Context, Error, CQ_BUFFERS, KL_CTX};

use crate::transport::cm;
use crate::transport::cm::{CmId, PreparedCmId};
use crate::transport::verbs;
use crate::transport::verbs::{CompletionQueue, WorkCompletion};

impl cm::Inner {
    #[inline]
    pub unsafe fn post_recv<T, R>(
        &self,
        mr: &mut verbs::MemoryRegion<T>,
        range: R,
        context: u64,
    ) -> Result<(), Error>
    where
        R: SliceIndex<[T], Output = [T]>,
    {
        let req = WorkRequest::PostRecv(
            self.handle.0,
            context,
            buf::Range::new(mr, range),
            mr.inner.0,
        );
        KL_CTX.with(|ctx| {
            // This WR must be successfully sent.
            let mut sent = false;
            while !sent {
                ctx.service.enqueue_wr_with(|ptr, count| {
                    debug_assert!(count >= 1);
                    ptr.cast::<WorkRequest>().write(req);
                    sent = true;
                    1
                })?;
                if !sent {
                    ctx.progress()?;
                }
            }
            Ok(())
        })
    }
}

impl PreparedCmId {
    /// # Safety
    ///
    /// The memory region can only be safely reused or dropped after the request is fully executed
    /// and a work completion has been retrieved from the corresponding completion queue (i.e.,
    /// until `CompletionQueue::poll_cq` returns a completion for this send).
    #[inline]
    pub unsafe fn post_recv<T, R>(
        &self,
        mr: &mut verbs::MemoryRegion<T>,
        range: R,
        context: u64,
    ) -> Result<(), Error>
    where
        R: SliceIndex<[T], Output = [T]>,
    {
        self.inner.post_recv(mr, range, context)
    }
}

impl CmId {
    /// # Safety
    ///
    /// The memory region can only be safely reused or dropped after the request is fully executed
    /// and a work completion has been retrieved from the corresponding completion queue (i.e.,
    /// until `CompletionQueue::poll_cq` returns a completion for this send).
    #[inline]
    pub unsafe fn post_recv<T, R>(
        &self,
        mr: &mut verbs::MemoryRegion<T>,
        range: R,
        context: u64,
    ) -> Result<(), Error>
    where
        R: SliceIndex<[T], Output = [T]>,
    {
        self.inner.post_recv(mr, range, context)
    }

    /// # Safety
    ///
    /// The memory region can only be safely reused or dropped after the request is fully executed
    /// and a work completion has been retrieved from the corresponding completion queue (i.e.,
    /// until `CompletionQueue::poll_cq` returns a completion for this send).
    #[inline]
    pub unsafe fn post_send<T, R>(
        &self,
        mr: &verbs::MemoryRegion<T>,
        range: R,
        context: u64,
        flags: verbs::SendFlags,
    ) -> Result<(), Error>
    where
        R: SliceIndex<[T], Output = [T]>,
    {
        let req = WorkRequest::PostSend(
            self.inner.handle.0,
            context,
            buf::Range::new(mr, range),
            mr.inner.0,
            flags,
        );
        KL_CTX.with(|ctx| {
            let mut sent = false;
            while !sent {
                ctx.service.enqueue_wr_with(|ptr, count| {
                    debug_assert!(count >= 1);
                    ptr.cast::<WorkRequest>().write(req);
                    sent = true;
                    1
                })?;
                if !sent {
                    ctx.progress()?;
                }
            }
            Ok(())
        })
    }

    /// # Safety
    ///
    /// The memory region can only be safely reused or dropped after the request is fully executed
    /// and a work completion has been retrieved from the corresponding completion queue (i.e.,
    /// until `CompletionQueue::poll_cq` returns a completion for this send).
    #[inline]
    pub unsafe fn post_write<T, R>(
        &self,
        mr: &verbs::MemoryRegion<T>,
        range: R,
        context: u64,
        flags: verbs::SendFlags,
        rkey: net::RemoteKey,
        remote_offset: u64,
    ) -> Result<(), Error>
    where
        R: SliceIndex<[T], Output = [T]>,
    {
        let req = WorkRequest::PostWrite(
            self.inner.handle.0,
            mr.inner.0,
            context,
            buf::Range::new(mr, range),
            remote_offset,
            rkey,
            flags,
        );
        KL_CTX.with(|ctx| {
            let mut sent = false;
            while !sent {
                ctx.service.enqueue_wr_with(|ptr, count| {
                    debug_assert!(count >= 1);
                    ptr.cast::<WorkRequest>().write(req);
                    sent = true;
                    1
                })?;
                if !sent {
                    ctx.progress()?;
                }
            }
            Ok(())
        })
    }

    /// # Safety
    ///
    /// The memory region can only be safely reused or dropped after the request is fully executed
    /// and a work completion has been retrieved from the corresponding completion queue (i.e.,
    /// until `CompletionQueue::poll_cq` returns a completion for this send).
    #[inline]
    pub unsafe fn post_read<T, R>(
        &self,
        mr: &mut verbs::MemoryRegion<T>,
        range: R,
        context: u64,
        flags: verbs::SendFlags,
        rkey: net::RemoteKey,
        remote_offset: u64,
    ) -> Result<(), Error>
    where
        R: SliceIndex<[T], Output = [T]>,
    {
        let req = WorkRequest::PostRead(
            self.inner.handle.0,
            mr.inner.0,
            context,
            buf::Range::new(mr, range),
            remote_offset,
            rkey,
            flags,
        );
        KL_CTX.with(|ctx| {
            let mut sent = false;
            while !sent {
                ctx.service.enqueue_wr_with(|ptr, count| {
                    debug_assert!(count >= 1);
                    ptr.cast::<WorkRequest>().write(req);
                    sent = true;
                    1
                })?;
                if !sent {
                    ctx.progress()?;
                }
            }
            Ok(())
        })
    }

    #[inline]
    pub fn get_send_comp(&self) -> Result<verbs::WorkCompletion, Error> {
        let mut wc = Vec::with_capacity(1);
        let cq = &self.inner.qp.send_cq;
        loop {
            cq.poll_cq(&mut wc)?;
            if wc.len() == 1 {
                break;
            }
        }
        Ok(wc[0])
    }

    #[inline]
    pub fn get_recv_comp(&self) -> Result<verbs::WorkCompletion, Error> {
        let mut wc = Vec::with_capacity(1);
        let cq = &self.inner.qp.recv_cq;
        loop {
            cq.poll_cq(&mut wc)?;
            if wc.len() == 1 {
                break;
            }
        }
        Ok(wc[0])
    }
}

impl Context {
    #[inline]
    fn progress(&self) -> Result<(), Error> {
        // Poll the shared memory queue, and put into the local buffer. This is called progress
        // because if the dp_cq is full, it may cause the program to hang unnecessarily.
        self.service.dequeue_wc_with(|ptr, count| unsafe {
            // iterate and dispatch
            let cq_buffers = CQ_BUFFERS.lock();
            for i in 0..count {
                let c = ptr.add(i).cast::<Completion>().read();
                if let Some(buffer) = cq_buffers.get(&c.cq_handle) {
                    buffer.shared.outstanding.store(false, Ordering::Release);
                    // this is just a notification that outstanding flag should be flapped
                    if c.wc.status != net::WcStatus::AGAIN {
                        buffer
                            .shared
                            .queue
                            .lock()
                            .push_back_checked(c.wc)
                            .expect("Something is wrong, local cq_buffer exceeds its limit");
                    }
                } else {
                    eprintln!("no corresponding entry for {:?}", c);
                }
            }
            count
        })?;
        Ok(())
    }
}

impl CompletionQueue {
    #[inline]
    pub fn poll_cq(&self, wc: &mut Vec<WorkCompletion>) -> Result<(), Error> {
        if wc.capacity() == 0 {
            return Ok(());
        }
        // poll local buffer first
        unsafe { wc.set_len(0) };
        let mut local_buffer = self.buffer.shared.queue.lock();
        if !local_buffer.is_empty() {
            let count = wc.capacity().min(local_buffer.len());
            for c in local_buffer.drain(..count) {
                wc.push(c);
            }
            return Ok(());
        }
        drop(local_buffer);

        // if local buffer is empty,
        KL_CTX.with(|ctx| {
            if !self.buffer.shared.outstanding.load(Ordering::Acquire) {
                // 1. Send a poll_cq command to the phoenix server. This poll_cq command does not have
                // to be sent successfully. Because the user would keep retrying until they get what
                // they expect.
                let req = WorkRequest::PollCq(self.inner);
                ctx.service.enqueue_wr_with(|ptr, _count| unsafe {
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
            ctx.progress()?;
            Ok(())
        })
    }
}
