//! Fast path operations.
use std::slice::SliceIndex;

use uapi::buf;

use super::{get_ops, Error};

use super::ucm;
use super::ucm::{CmId, PreparedCmId};
use super::uverbs;
use super::uverbs::{CompletionQueue, WorkCompletion};

impl ucm::Inner {
    #[inline]
    pub(crate) unsafe fn post_recv<T, R>(
        &self,
        mr: &mut uverbs::MemoryRegion<T>,
        range: R,
        context: u64,
    ) -> Result<(), Error>
    where
        R: SliceIndex<[T], Output = [T]>,
    {
        get_ops().post_recv(
            self.handle.0,
            &mr.inner.mr,
            buf::Range::new(mr, range),
            context,
        )?;
        Ok(())
    }
}

impl PreparedCmId {
    /// # Safety
    ///
    /// The memory region can only be safely reused or dropped after the request is fully executed
    /// and a work completion has been retrieved from the corresponding completion queue (i.e.,
    /// until `CompletionQueue::poll_cq` returns a completion for this send).
    #[inline]
    pub(crate) unsafe fn post_recv<T, R>(
        &self,
        mr: &mut uverbs::MemoryRegion<T>,
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
    pub(crate) unsafe fn post_recv<T, R>(
        &self,
        mr: &mut uverbs::MemoryRegion<T>,
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
    pub(crate) unsafe fn post_send<T, R>(
        &self,
        mr: &uverbs::MemoryRegion<T>,
        range: R,
        context: u64,
        flags: uverbs::SendFlags,
    ) -> Result<(), Error>
    where
        R: SliceIndex<[T], Output = [T]>,
    {
        get_ops().post_send(
            self.inner.handle.0,
            &mr.inner.mr,
            buf::Range::new(mr, range),
            context,
            flags,
        )?;
        Ok(())
    }

    /// # Safety
    ///
    /// The memory region can only be safely reused or dropped after the request is fully executed
    /// and a work completion has been retrieved from the corresponding completion queue (i.e.,
    /// until `CompletionQueue::poll_cq` returns a completion for this send).
    #[inline]
    pub(crate) unsafe fn post_send_with_imm<T, R>(
        &self,
        mr: &uverbs::MemoryRegion<T>,
        range: R,
        context: u64,
        flags: uverbs::SendFlags,
        imm: u32,
    ) -> Result<(), Error>
    where
        R: SliceIndex<[T], Output = [T]>,
    {
        get_ops().post_send_with_imm(
            self.inner.handle.0,
            &mr.inner.mr,
            buf::Range::new(mr, range),
            context,
            flags,
            imm,
        )?;
        Ok(())
    }

    /// # Safety
    ///
    /// The memory region can only be safely reused or dropped after the request is fully executed
    /// and a work completion has been retrieved from the corresponding completion queue (i.e.,
    /// until `CompletionQueue::poll_cq` returns a completion for this send).
    #[inline]
    pub(crate) unsafe fn post_write<T, R>(
        &self,
        mr: &uverbs::MemoryRegion<T>,
        range: R,
        context: u64,
        flags: uverbs::SendFlags,
        rkey: uapi::net::RemoteKey,
        remote_offset: u64,
    ) -> Result<(), Error>
    where
        R: SliceIndex<[T], Output = [T]>,
    {
        get_ops().post_write(
            self.inner.handle.0,
            &mr.inner.mr,
            buf::Range::new(mr, range),
            context,
            rkey,
            remote_offset,
            flags,
        )?;
        Ok(())
    }

    /// # Safety
    ///
    /// The memory region can only be safely reused or dropped after the request is fully executed
    /// and a work completion has been retrieved from the corresponding completion queue (i.e.,
    /// until `CompletionQueue::poll_cq` returns a completion for this send).
    #[inline]
    pub(crate) unsafe fn post_read<T, R>(
        &self,
        mr: &mut uverbs::MemoryRegion<T>,
        range: R,
        context: u64,
        flags: uverbs::SendFlags,
        rkey: uapi::net::RemoteKey,
        remote_offset: u64,
    ) -> Result<(), Error>
    where
        R: SliceIndex<[T], Output = [T]>,
    {
        get_ops().post_read(
            self.inner.handle.0,
            &mr.inner.mr,
            buf::Range::new(mr, range),
            context,
            rkey,
            remote_offset,
            flags,
        )?;
        Ok(())
    }

    #[inline]
    pub(crate) fn get_send_comp(&self) -> Result<uverbs::WorkCompletion, Error> {
        let mut wc = Vec::with_capacity(1);
        let cq = &self.inner.qp.send_cq;
        loop {
            cq.poll(&mut wc)?;
            if wc.len() == 1 {
                break;
            }
        }
        Ok(wc[0])
    }

    #[inline]
    pub(crate) fn get_recv_comp(&self) -> Result<uverbs::WorkCompletion, Error> {
        let mut wc = Vec::with_capacity(1);
        let cq = &self.inner.qp.recv_cq;
        loop {
            cq.poll(&mut wc)?;
            if wc.len() == 1 {
                break;
            }
        }
        Ok(wc[0])
    }
}

impl CompletionQueue {
    #[inline]
    pub(crate) fn poll(&self, wc: &mut Vec<WorkCompletion>) -> Result<(), Error> {
        get_ops().poll_cq(&self.inner, wc)?;
        Ok(())
    }
}
