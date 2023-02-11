use std::any::Any;
use std::borrow::Borrow;
use std::marker::PhantomData;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::slice;

use phoenix_common::log;
use rdma::mr::OdpMemoryRegion;
use rdma::rdmacm;
use uapi::net;
use uapi::net::returned;
use uapi::{AsHandle, Handle};

use super::{get_ops, Error, FromBorrow};

// Re-exports
pub use uapi::net::{AccessFlags, SendFlags, WcFlags, WcOpcode, WcStatus, WorkCompletion};
pub use uapi::net::{QpCapability, QpType, RemoteKey};

pub(crate) fn get_default_verbs_contexts() -> Result<Vec<VerbsContext>, Error> {
    let ctx_list = get_ops().get_default_contexts()?;
    ctx_list
        .into_iter()
        .map(VerbsContext::new)
        .collect::<Result<Vec<_>, Error>>()
}

pub(crate) fn get_default_pds() -> Result<Vec<ProtectionDomain>, Error> {
    // This should only be called when it is first initialized. At that time, hopefully KL_CTX has
    // already been initialized.
    let pds = get_ops().get_default_pds()?;
    pds.into_iter()
        .map(ProtectionDomain::open)
        .collect::<Result<Vec<_>, Error>>()
}

#[derive(Debug)]
pub struct VerbsContext {
    pub(crate) inner: net::VerbsContext,
}

impl AsHandle for VerbsContext {
    fn as_handle(&self) -> Handle {
        self.inner.0
    }
}

// Default verbs contexts are 'static, no need to drop and open them

impl VerbsContext {
    pub(crate) fn new(verbs: returned::VerbsContext) -> Result<Self, Error> {
        Ok(VerbsContext {
            inner: verbs.handle,
        })
    }

    pub(crate) fn create_cq(
        &self,
        min_cq_entries: i32,
        cq_context: u64,
    ) -> Result<CompletionQueue, Error> {
        let cq = get_ops().create_cq(&self.inner, min_cq_entries, cq_context)?;
        CompletionQueue::open(cq)
    }
}

#[derive(Debug)]
pub struct ProtectionDomain {
    pub(crate) inner: net::ProtectionDomain,
}

impl Drop for ProtectionDomain {
    fn drop(&mut self) {
        get_ops()
            .dealloc_pd(&self.inner)
            .unwrap_or_else(|e| eprintln!("Dropping ProtectionDomain: {}", e));
    }
}

impl ProtectionDomain {
    pub(crate) fn open(pd: returned::ProtectionDomain) -> Result<Self, Error> {
        let inner = pd.handle;
        get_ops().open_pd(&inner)?;
        Ok(ProtectionDomain { inner })
    }

    pub(crate) fn allocate<T: Sized + Copy>(
        &self,
        _len: usize,
        _access: net::AccessFlags,
    ) -> Result<MemoryRegion<T>, Error> {
        todo!()
        // let nbytes = len * mem::size_of::<T>();
        // assert!(nbytes > 0);
        // let mr = get_ops().reg_mr(&self.inner, nbytes, access)?;

        // assert_eq!(nbytes, mr.len());
        // Ok(MemoryRegion::new(mr)?)
    }
}

#[derive(Debug)]
pub struct CompletionQueue {
    pub(crate) inner: net::CompletionQueue,
}

impl AsHandle for CompletionQueue {
    fn as_handle(&self) -> Handle {
        self.inner.0
    }
}

impl Drop for CompletionQueue {
    fn drop(&mut self) {
        get_ops()
            .destroy_cq(&self.inner)
            .unwrap_or_else(|e| eprintln!("Dropping CompletionQueue: {}", e));
    }
}

impl CompletionQueue {
    pub(crate) fn open(returned_cq: returned::CompletionQueue) -> Result<Self, Error> {
        let inner = returned_cq.handle;
        get_ops().open_cq(&inner)?;
        Ok(CompletionQueue { inner })
    }

    pub(crate) fn get_verbs_context(&self) -> Result<VerbsContext, Error> {
        let returned_ctx = get_ops().get_verbs_for_cq(&self.inner)?;
        VerbsContext::new(returned_ctx)
    }
}

pub struct SharedReceiveQueue {
    pub(crate) inner: net::SharedReceiveQueue,
}

#[derive(Debug)]
pub struct MemoryRegion<T> {
    pub(crate) inner: OdpMemoryRegion,
    _marker: PhantomData<T>,
}

impl<T> Deref for MemoryRegion<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        unsafe {
            slice::from_raw_parts(
                self.inner.as_ptr().cast(),
                self.inner.len() / mem::size_of::<T>(),
            )
        }
    }
}

impl<T> DerefMut for MemoryRegion<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            slice::from_raw_parts_mut(
                self.inner.as_mut_ptr().cast(),
                self.inner.len() / mem::size_of::<T>(),
            )
        }
    }
}

impl<T> AsHandle for MemoryRegion<T> {
    fn as_handle(&self) -> Handle {
        self.inner.as_handle()
    }
}

impl<T: Sized + Copy> MemoryRegion<T> {
    pub(crate) fn new(mr: rdmacm::MemoryRegion<'static>) -> Result<Self, Error> {
        Ok(MemoryRegion {
            inner: OdpMemoryRegion::new(mr),
            _marker: PhantomData,
        })
    }

    #[inline]
    pub(crate) fn as_slice(&self) -> &[T] {
        self
    }

    #[inline]
    pub(crate) fn as_mut_slice(&mut self) -> &mut [T] {
        self
    }

    // #[inline]
    // pub(crate) fn rkey(&self) -> RemoteKey {
    //     self.inner.rkey()
    // }

    // #[inline]
    // pub fn pd(&self) -> &ProtectionDomain {
    //     &self.inner.pd()
    // }

    #[inline]
    pub(crate) fn as_ptr(&self) -> *const T {
        self.inner.as_ptr().cast()
    }

    #[inline]
    pub(crate) fn as_mut_ptr(&mut self) -> *mut T {
        self.inner.as_mut_ptr().cast()
    }
}

#[derive(Debug)]
pub struct QueuePair {
    pub(crate) inner: net::QueuePair,
    pub(crate) pd: ProtectionDomain,
    pub(crate) send_cq: CompletionQueue,
    pub(crate) recv_cq: CompletionQueue,
}

impl QueuePair {
    pub(crate) fn open(returned_qp: returned::QueuePair) -> Result<Self, Error> {
        let inner = returned_qp.handle;
        get_ops().open_qp(&inner)?;
        Ok(QueuePair {
            inner,
            pd: ProtectionDomain::open(returned_qp.pd)?,
            send_cq: CompletionQueue::open(returned_qp.send_cq)?,
            recv_cq: CompletionQueue::open(returned_qp.recv_cq)?,
        })
    }

    #[inline]
    pub(crate) fn pd(&self) -> &ProtectionDomain {
        &self.pd
    }

    #[inline]
    pub(crate) fn send_cq(&self) -> &CompletionQueue {
        &self.send_cq
    }

    #[inline]
    pub(crate) fn recv_cq(&self) -> &CompletionQueue {
        &self.recv_cq
    }
}

impl Drop for QueuePair {
    fn drop(&mut self) {
        log::debug!("dropping QueuePair");
        get_ops()
            .destroy_qp(&self.inner)
            .unwrap_or_else(|e| eprintln!("Dropping QueuePair: {}", e));
        log::debug!("dropped QueuePair");
    }
}

#[derive(Clone)]
pub(crate) struct QpInitAttr<'ctx, 'scq, 'rcq, 'srq> {
    pub(crate) qp_context: Option<&'ctx (dyn Any + Send + Sync)>,
    pub(crate) send_cq: Option<&'scq CompletionQueue>,
    pub(crate) recv_cq: Option<&'rcq CompletionQueue>,
    pub(crate) srq: Option<&'srq SharedReceiveQueue>,
    pub(crate) cap: QpCapability,
    pub(crate) qp_type: QpType,
    pub(crate) sq_sig_all: bool,
}

impl<'ctx, 'scq, 'rcq, 'srq> Default for QpInitAttr<'ctx, 'scq, 'rcq, 'srq> {
    fn default() -> Self {
        QpInitAttr {
            qp_context: None,
            send_cq: None,
            recv_cq: None,
            srq: None,
            cap: QpCapability {
                max_send_wr: 1,
                max_recv_wr: 1,
                max_send_sge: 1,
                max_recv_sge: 1,
                max_inline_data: 128,
            },
            qp_type: QpType::RC,
            sq_sig_all: false,
        }
    }
}

impl<'ctx, 'scq, 'rcq, 'srq> FromBorrow<QpInitAttr<'ctx, 'scq, 'rcq, 'srq>> for net::QpInitAttr {
    fn from_borrow<T: Borrow<QpInitAttr<'ctx, 'scq, 'rcq, 'srq>>>(borrow: &T) -> Self {
        let b = borrow.borrow();
        net::QpInitAttr {
            send_cq: b.send_cq.map(|x| x.inner),
            recv_cq: b.recv_cq.map(|x| x.inner),
            srq: b.srq.map(|x| x.inner),
            cap: b.cap,
            qp_type: b.qp_type,
            sq_sig_all: b.sq_sig_all,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ConnParam<'priv_data> {
    pub(crate) private_data: Option<&'priv_data [u8]>,
    pub(crate) responder_resources: u8,
    pub(crate) initiator_depth: u8,
    pub(crate) flow_control: u8,
    pub(crate) retry_count: u8,
    pub(crate) rnr_retry_count: u8,
    pub(crate) srq: u8,
    pub(crate) qp_num: u32,
}

impl<'priv_data> FromBorrow<ConnParam<'priv_data>> for net::ConnParam {
    fn from_borrow<T: Borrow<ConnParam<'priv_data>>>(borrow: &T) -> Self {
        let b = borrow.borrow();
        net::ConnParam {
            private_data: b.private_data.map(|x| x.to_owned()),
            responder_resources: b.responder_resources,
            initiator_depth: b.initiator_depth,
            flow_control: b.flow_control,
            retry_count: b.retry_count,
            rnr_retry_count: b.rnr_retry_count,
            srq: b.srq,
            qp_num: b.qp_num,
        }
    }
}
