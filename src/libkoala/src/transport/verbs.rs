use std::any::Any;
use std::borrow::Borrow;
use std::collections::VecDeque;
use std::io;
use std::marker::PhantomData;
use std::mem;
use std::ops::{Deref, DerefMut};
use std::os::unix::io::AsRawFd;
use std::slice;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use lazy_static::lazy_static;
use memfd::Memfd;
use memmap2::{MmapOptions, MmapRaw};

use interface::returned;
use ipc::transport::rdma::cmd::{Command, CompletionKind};

use crate::transport::{Error, CQ_BUFFERS, KL_CTX};
use crate::{rx_recv_impl, FromBorrow};

// Re-exports
pub use interface::{AccessFlags, SendFlags, WcFlags, WcOpcode, WcStatus, WorkCompletion};
pub use interface::{QpCapability, QpType, RemoteKey};

lazy_static! {
    pub static ref DEFAULT_PDS: Vec<ProtectionDomain> =
        get_default_pds().expect("Failed to get default PDs");
}

fn get_default_pds() -> Result<Vec<ProtectionDomain>, Error> {
    // This should only be called when it is first initialized. At that time, hopefully KL_CTX has
    // already been initialized.
    KL_CTX.with(|ctx| {
        let req = Command::GetDefaultPds;
        ctx.service.send_cmd(req)?;
        rx_recv_impl!(ctx.service, CompletionKind::GetDefaultPds, pds, {
            pds.into_iter()
                .map(|pd| ProtectionDomain::open(pd))
                .collect::<Result<Vec<_>, Error>>()
        })
    })
}

#[derive(Debug)]
pub struct ProtectionDomain {
    pub(crate) inner: interface::ProtectionDomain,
}

impl Drop for ProtectionDomain {
    fn drop(&mut self) {
        (|| {
            let req = Command::DeallocPd(self.inner);
            KL_CTX.with(|ctx| {
                ctx.service.send_cmd(req)?;
                rx_recv_impl!(ctx.service, CompletionKind::DeallocPd)
            })
        })()
        .unwrap_or_else(|e| eprintln!("Dropping ProtectionDomain: {}", e));
    }
}

impl ProtectionDomain {
    pub(crate) fn open(pd: returned::ProtectionDomain) -> Result<Self, Error> {
        KL_CTX.with(|ctx| {
            let inner = pd.handle;
            let req = Command::OpenPd(inner);
            ctx.service.send_cmd(req)?;
            rx_recv_impl!(ctx.service, CompletionKind::OpenPd, {
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
        let req = Command::RegMr(self.inner, nbytes, access);
        KL_CTX.with(|ctx| {
            ctx.service.send_cmd(req)?;
            let fds = ctx.service.recv_fd()?;

            assert_eq!(fds.len(), 1);

            let memfd = Memfd::try_from_fd(fds[0]).map_err(|_| io::Error::last_os_error())?;
            let file_len = memfd.as_file().metadata()?.len() as usize;
            assert_eq!(file_len, nbytes);

            rx_recv_impl!(ctx.service, CompletionKind::RegMr, mr, {
                MemoryRegion::new(self.inner, mr.handle, mr.rkey, mr.vaddr, memfd)
            })
        })
    }

    #[inline]
    pub fn default_pds() -> &'static [ProtectionDomain] {
        &DEFAULT_PDS
    }
}

#[derive(Debug)]
pub struct CompletionQueue {
    pub(crate) inner: interface::CompletionQueue,
    pub(crate) outstanding: AtomicBool,
    pub(crate) buffer: CqBuffer,
}

#[derive(Debug)]
pub(crate) struct CqBuffer {
    pub(crate) queue: Arc<spin::Mutex<VecDeque<WorkCompletion>>>,
}

impl Clone for CqBuffer {
    fn clone(&self) -> Self {
        CqBuffer {
            queue: Arc::clone(&self.queue),
        }
    }
}

impl CqBuffer {
    fn new() -> Self {
        CqBuffer {
            queue: Arc::new(spin::Mutex::new(VecDeque::new())),
        }
    }

    #[inline]
    fn refcnt(&self) -> usize {
        Arc::strong_count(&self.queue)
    }
}

impl Drop for CompletionQueue {
    fn drop(&mut self) {
        (|| {
            let mut cq_buffers = CQ_BUFFERS.lock();
            let ref_cnt = cq_buffers[&self.inner].refcnt();
            if ref_cnt == 2 {
                // this is the last CQ
                // should I flush the remaining completions in the buffer?
                cq_buffers.remove(&self.inner);
            }
            drop(cq_buffers);

            let req = Command::DestroyCq(self.inner);
            KL_CTX.with(|ctx| {
                ctx.service.send_cmd(req)?;
                rx_recv_impl!(ctx.service, CompletionKind::DestroyCq)
            })
        })()
        .unwrap_or_else(|e| eprintln!("Dropping CompletionQueue: {}", e));
    }
}

impl CompletionQueue {
    pub(crate) fn open(returned_cq: returned::CompletionQueue) -> Result<Self, Error> {
        // allocate a buffer in the thread local context
        KL_CTX.with(|ctx| {
            let buffer = CQ_BUFFERS
                .lock()
                .entry(returned_cq.handle)
                .or_insert_with(CqBuffer::new)
                .clone();
            let inner = returned_cq.handle;
            let req = Command::OpenCq(inner);
            ctx.service.send_cmd(req)?;
            rx_recv_impl!(ctx.service, CompletionKind::OpenCq, {
                Ok(CompletionQueue {
                    inner,
                    outstanding: AtomicBool::new(false),
                    buffer,
                })
            })
        })
    }
}

pub struct SharedReceiveQueue {
    pub(crate) inner: interface::SharedReceiveQueue,
}

#[derive(Debug)]
pub struct MemoryRegion<T> {
    pub(crate) inner: interface::MemoryRegion,
    mmap: MmapRaw,
    // mmap: Mmap,
    rkey: RemoteKey,
    // offset between the remote mapped shared memory address and the local shared memory in bytes
    pub(crate) remote_addr: u64,
    _memfd: Memfd,
    _pd: ProtectionDomain,
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
        (|| {
            KL_CTX.with(|ctx| {
                let req = Command::DeregMr(self.inner);
                ctx.service.send_cmd(req)?;
                rx_recv_impl!(ctx.service, CompletionKind::DeregMr)
            })
        })()
        .unwrap_or_else(|e| eprintln!("Dropping MemoryRegion: {}", e));
    }
}

use nix::sys::mman::{mmap, munmap, MapFlags, ProtFlags};
use std::fs;
pub struct Mmap {
    ptr: *mut libc::c_void,
    len: usize,
}

impl Drop for Mmap {
    fn drop(&mut self) {
        unsafe {
            munmap(self.ptr, self.len).unwrap_or_else(|e| eprintln!("failed to munmap: {}", e))
        };
    }
}

impl Mmap {
    fn new(target_addr: usize, memfile: &fs::File) -> io::Result<Self> {
        let len = memfile.metadata()?.len() as usize;
        // TODO(cjr): use MAP_FIXED_NOREPLACE
        let ptr = unsafe {
            mmap(
                target_addr as *mut libc::c_void,
                len,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED | MapFlags::MAP_NORESERVE | MapFlags::MAP_FIXED,
                memfile.as_raw_fd(),
                0,
            )?
        };
        assert_eq!(ptr as usize, target_addr);
        Ok(Self { ptr, len })
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns a raw pointer to the memory mapped file.
    ///
    /// Before dereferencing this pointer, you have to make sure that the file has not been
    /// truncated since the memory map was created.
    /// Avoiding this will not introduce memory safety issues in Rust terms,
    /// but will cause SIGBUS (or equivalent) signal.
    #[inline]
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr as *const u8
    }

    /// Returns an unsafe mutable pointer to the memory mapped file.
    ///
    /// Before dereferencing this pointer, you have to make sure that the file has not been
    /// truncated since the memory map was created.
    /// Avoiding this will not introduce memory safety issues in Rust terms,
    /// but will cause SIGBUS (or equivalent) signal.
    #[inline]
    pub fn as_mut_ptr(&self) -> *mut u8 {
        self.ptr as *mut u8
    }
}

// Why this is safe?
unsafe impl Sync for Mmap {}
unsafe impl Send for Mmap {}

impl Deref for Mmap {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.as_ptr(), self.len()) }
    }
}

impl AsRef<[u8]> for Mmap {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

impl DerefMut for Mmap {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.as_mut_ptr(), self.len()) }
    }
}

use std::fmt;
impl fmt::Debug for Mmap {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Mmap")
            .field("ptr", &self.as_ptr())
            .field("len", &self.len())
            .finish()
    }
}


impl<T: Sized + Copy> MemoryRegion<T> {
    pub(crate) fn new(
        pd: interface::ProtectionDomain,
        inner: interface::MemoryRegion,
        rkey: RemoteKey,
        remote_addr: u64,
        memfd: Memfd,
    ) -> Result<Self, Error> {
        // Map to the same address as remote_addr, panic if it does not work
        // TODO(cjr): will design a mechanism to make sure the uniqueness of addresses in the future
        // let mmap = Mmap::new(remote_addr as _, memfd.as_file())?;
        let mmap = MmapOptions::new().map_raw(memfd.as_file())?;
        Ok(MemoryRegion {
            inner,
            rkey,
            mmap,
            remote_addr,
            _pd: ProtectionDomain::open(returned::ProtectionDomain { handle: pd })?,
            _memfd: memfd,
            _marker: PhantomData,
        })
    }

    #[inline]
    pub fn as_slice(&self) -> &[T] {
        self
    }

    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        self
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

#[derive(Debug)]
pub struct QueuePair {
    pub(crate) inner: interface::QueuePair,
    pub(crate) pd: ProtectionDomain,
    pub(crate) send_cq: CompletionQueue,
    pub(crate) recv_cq: CompletionQueue,
}

impl QueuePair {
    pub(crate) fn open(returned_qp: returned::QueuePair) -> Result<Self, Error> {
        KL_CTX.with(|ctx| {
            let inner = returned_qp.handle;
            let req = Command::OpenQp(inner);
            ctx.service.send_cmd(req)?;
            rx_recv_impl!(ctx.service, CompletionKind::OpenQp, {
                Ok(QueuePair {
                    inner,
                    pd: ProtectionDomain::open(returned_qp.pd)?,
                    send_cq: CompletionQueue::open(returned_qp.send_cq)?,
                    recv_cq: CompletionQueue::open(returned_qp.recv_cq)?,
                })
            })
        })
    }

    #[inline]
    pub fn pd(&self) -> &ProtectionDomain {
        &self.pd
    }

    #[inline]
    pub fn send_cq(&self) -> &CompletionQueue {
        &self.send_cq
    }

    #[inline]
    pub fn recv_cq(&self) -> &CompletionQueue {
        &self.recv_cq
    }
}

impl Drop for QueuePair {
    fn drop(&mut self) {
        (|| {
            let req = Command::DestroyQp(self.inner);
            KL_CTX.with(|ctx| {
                ctx.service.send_cmd(req)?;
                rx_recv_impl!(ctx.service, CompletionKind::DestroyQp)
            })
        })()
        .unwrap_or_else(|e| eprintln!("Dropping QueuePair: {}", e));
    }
}

#[derive(Clone)]
pub struct QpInitAttr<'ctx, 'scq, 'rcq, 'srq> {
    pub qp_context: Option<&'ctx dyn Any>,
    pub send_cq: Option<&'scq CompletionQueue>,
    pub recv_cq: Option<&'rcq CompletionQueue>,
    pub srq: Option<&'srq SharedReceiveQueue>,
    pub cap: QpCapability,
    pub qp_type: QpType,
    pub sq_sig_all: bool,
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

impl<'ctx, 'scq, 'rcq, 'srq> FromBorrow<QpInitAttr<'ctx, 'scq, 'rcq, 'srq>>
    for interface::QpInitAttr
{
    fn from_borrow<T: Borrow<QpInitAttr<'ctx, 'scq, 'rcq, 'srq>>>(borrow: &T) -> Self {
        let b = borrow.borrow();
        interface::QpInitAttr {
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
