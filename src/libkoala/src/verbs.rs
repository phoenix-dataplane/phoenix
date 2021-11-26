use std::any::Any;
use std::borrow::Borrow;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fs::File;
use std::io;
use std::marker::PhantomData;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::os::unix::io::FromRawFd;
use std::rc::Rc;
use std::slice;
use std::sync::atomic::AtomicBool;

use memfd::Memfd;
use memmap2::{MmapOptions, MmapRaw};

use interface::returned;
use ipc::cmd::{Request, ResponseKind};

use crate::{rx_recv_impl, Error, FromBorrow, KL_CTX};

// Re-exports
pub use interface::{AccessFlags, SendFlags, WcFlags, WcOpcode, WcStatus, WorkCompletion};
pub use interface::{QpCapability, QpType, RemoteKey};

pub struct ProtectionDomain {
    pub(crate) inner: interface::ProtectionDomain,
}

impl Drop for ProtectionDomain {
    fn drop(&mut self) {
        (|| {
            let req = Request::DeallocPd(self.inner);
            KL_CTX.with(|ctx| {
                ctx.cmd_tx.send(req)?;
                rx_recv_impl!(ctx.cmd_rx, ResponseKind::DeallocPd)
            })
        })()
        .unwrap_or_else(|e| eprintln!("Dropping ProtectionDomain: {}", e));
    }
}

impl ProtectionDomain {
    pub(crate) fn open(pd: returned::ProtectionDomain) -> Result<Self, Error> {
        KL_CTX.with(|ctx| {
            let inner = pd.handle;
            let req = Request::OpenPd(inner);
            ctx.cmd_tx.send(req)?;
            rx_recv_impl!(ctx.cmd_rx, ResponseKind::OpenPd, {
                Ok(ProtectionDomain { inner })
            })
        })
    }

    pub fn allocate<T: Sized + Copy>(
        &self,
        len: usize,
        access: interface::AccessFlags,
    ) -> Result<MemoryRegion<T>, Error> {
        let nbytes = len * mem::size_of::<T>();
        assert!(nbytes > 0);
        let req = Request::RegMr(self.inner, nbytes, access);
        KL_CTX.with(|ctx| {
            ctx.cmd_tx.send(req)?;
            let fds = ipc::recv_fd(&ctx.sock)?;

            assert_eq!(fds.len(), 1);

            let file = unsafe { File::from_raw_fd(fds[0]) };
            let file_len = file.metadata()?.len() as usize;
            assert_eq!(file_len, nbytes);

            rx_recv_impl!(ctx.cmd_rx, ResponseKind::RegMr, mr, {
                MemoryRegion::new(mr.handle, mr.rkey, file)
            })
        })
    }
}

pub struct CompletionQueue {
    pub(crate) inner: interface::CompletionQueue,
    pub(crate) buffer: CqBuffer,
}

#[derive(Debug)]
pub(crate) struct CqBufferShared {
    pub(crate) outstanding: AtomicBool,
    pub(crate) queue: RefCell<VecDeque<WorkCompletion>>,
}

#[derive(Debug)]
pub(crate) struct CqBuffer {
    pub(crate) shared: Rc<CqBufferShared>,
}

impl CqBuffer {
    fn new() -> Self {
        CqBuffer {
            shared: Rc::new(CqBufferShared {
                outstanding: AtomicBool::new(false),
                queue: RefCell::new(VecDeque::new()),
            }),
        }
    }
}

impl Drop for CompletionQueue {
    fn drop(&mut self) {
        (|| {
            KL_CTX.with(|ctx| {
                let ref_cnt = Rc::strong_count(&ctx.cq_buffers.borrow()[&self.inner].shared);
                if ref_cnt == 2 {
                    // this is the last CQ
                    // should I flush the remaining completions in the buffer?
                    ctx.cq_buffers.borrow_mut().remove(&self.inner);
                }

                let req = Request::DestroyCq(self.inner);
                KL_CTX.with(|ctx| {
                    ctx.cmd_tx.send(req)?;
                    rx_recv_impl!(ctx.cmd_rx, ResponseKind::DestroyCq)
                })
            })
        })()
        .unwrap_or_else(|e| eprintln!("Dropping CompletionQueue: {}", e));
    }
}

impl CompletionQueue {
    pub(crate) fn open(returned_cq: returned::CompletionQueue) -> Result<Self, Error> {
        // allocate a buffer in the thread local context
        KL_CTX.with(|ctx| {
            let shared = Rc::clone(
                &ctx.cq_buffers
                    .borrow_mut()
                    .entry(returned_cq.handle)
                    .or_insert_with(CqBuffer::new)
                    .shared,
            );
            let inner = returned_cq.handle;
            let req = Request::OpenCq(inner);
            ctx.cmd_tx.send(req)?;
            rx_recv_impl!(ctx.cmd_rx, ResponseKind::OpenCq, {
                Ok(CompletionQueue {
                    inner,
                    buffer: CqBuffer { shared },
                })
            })
        })
    }
}

// TODO(cjr): For the moment, we disallow `Send` for CompletionQueue.
impl !Send for CompletionQueue {}
impl !Sync for CompletionQueue {}

pub struct SharedReceiveQueue {
    pub(crate) inner: interface::SharedReceiveQueue,
}

#[derive(Debug)]
pub struct MemoryRegion<T> {
    pub(crate) inner: interface::MemoryRegion,
    mmap: MmapRaw,
    memfd: Memfd,
    rkey: RemoteKey,
    _marker: PhantomData<T>,
}

impl<T> Deref for MemoryRegion<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        unsafe {
            slice::from_raw_parts(
                self.mmap.as_ptr().cast(),
                self.mmap.len() / mem::size_of::<T>(),
            )
        }
    }
}

impl<T> DerefMut for MemoryRegion<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {
            slice::from_raw_parts_mut(
                self.mmap.as_mut_ptr().cast(),
                self.mmap.len() / mem::size_of::<T>(),
            )
        }
    }
}

impl<T> Drop for MemoryRegion<T> {
    fn drop(&mut self) {
        (|| KL_CTX.with(|ctx| {
            let req = Request::DeregMr(self.inner);
            ctx.cmd_tx.send(req)?;
            rx_recv_impl!(ctx.cmd_rx, ResponseKind::DeregMr)
        }))().unwrap_or_else(|e| eprintln!("Dropping MemoryRegion: {}", e));
    }
}

impl<T: Sized + Copy> MemoryRegion<T> {
    fn new(inner: interface::MemoryRegion, rkey: RemoteKey, file: File) -> Result<Self, Error> {
        let memfd = Memfd::try_from_file(file).map_err(|_| io::Error::last_os_error())?;
        let mmap = MmapOptions::new().map_raw(memfd.as_file())?;
        Ok(MemoryRegion {
            inner,
            rkey,
            mmap,
            memfd,
            _marker: PhantomData,
        })
    }

    #[inline]
    pub fn rkey(&self) -> RemoteKey {
        self.rkey
    }

    #[inline]
    pub fn as_ptr(&self) -> *const T {
        self.mmap.as_ptr() as *const T
    }

    #[inline]
    pub fn as_mut_ptr(&self) -> *mut T {
        self.mmap.as_mut_ptr() as *mut T
    }
}

pub struct QueuePair {
    pub(crate) inner: interface::QueuePair,
    pub pd: ProtectionDomain,
    pub send_cq: CompletionQueue,
    pub recv_cq: CompletionQueue,
}

impl QueuePair {
    pub(crate) fn open(returned_qp: returned::QueuePair) -> Result<Self, Error> {
        KL_CTX.with(|ctx| {
            let inner = returned_qp.handle;
            let req = Request::OpenQp(inner);
            ctx.cmd_tx.send(req)?;
            rx_recv_impl!(ctx.cmd_rx, ResponseKind::OpenQp, {
                Ok(QueuePair {
                    inner,
                    pd: ProtectionDomain::open(returned_qp.pd)?,
                    send_cq: CompletionQueue::open(returned_qp.send_cq)?,
                    recv_cq: CompletionQueue::open(returned_qp.recv_cq)?,
                })
            })
        })
    }
}

impl Drop for QueuePair {
    fn drop(&mut self) {
        (|| {
            let req = Request::DestroyQp(self.inner);
            KL_CTX.with(|ctx| {
                ctx.cmd_tx.send(req)?;
                rx_recv_impl!(ctx.cmd_rx, ResponseKind::DestroyQp)
            })
        })()
        .unwrap_or_else(|e| eprintln!("Dropping QueuePair: {}", e));
    }
}

pub struct QpInitAttr<'ctx> {
    pub qp_context: Option<&'ctx dyn Any>,
    pub send_cq: Option<CompletionQueue>,
    pub recv_cq: Option<CompletionQueue>,
    pub srq: Option<SharedReceiveQueue>,
    pub cap: QpCapability,
    pub qp_type: QpType,
    pub sq_sig_all: bool,
}

impl<'ctx> FromBorrow<QpInitAttr<'ctx>> for interface::QpInitAttr {
    fn from_borrow<T: Borrow<QpInitAttr<'ctx>>>(borrow: &T) -> Self {
        let b = borrow.borrow();
        interface::QpInitAttr {
            send_cq: b.send_cq.as_ref().map(|x| x.inner.clone()),
            recv_cq: b.recv_cq.as_ref().map(|x| x.inner.clone()),
            srq: b.srq.as_ref().map(|x| x.inner.clone()),
            cap: b.cap.clone(),
            qp_type: b.qp_type,
            sq_sig_all: b.sq_sig_all,
        }
    }
}

pub struct ConnParam<'priv_data> {
    pub private_data: Option<&'priv_data [u8]>,
    pub responder_resources: u8,
    pub initiator_depth: u8,
    pub flow_control: u8,
    pub retry_count: u8,
    pub rnr_retry_count: u8,
    pub srq: u8,
    pub qp_num: u32,
}

impl<'priv_data> FromBorrow<ConnParam<'priv_data>> for interface::ConnParam {
    fn from_borrow<T: Borrow<ConnParam<'priv_data>>>(borrow: &T) -> Self {
        let b = borrow.borrow();
        interface::ConnParam {
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
