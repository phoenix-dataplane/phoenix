use std::fs::File;
use std::io;
use std::ops::Range;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::rc::Rc;

use ipc::cmd::{Request, ResponseKind};

use crate::{slice_to_range, verbs, Context, Error, FromBorrow};

// Re-exports
pub use interface::addrinfo::{AddrInfo, AddrInfoHints};

/// Address and route resolution service.
pub fn getaddrinfo(
    ctx: &Context,
    node: Option<&str>,
    service: Option<&str>,
    hints: Option<&AddrInfoHints>,
) -> Result<AddrInfo, Error> {
    let req = Request::GetAddrInfo(
        node.map(String::from),
        service.map(String::from),
        hints.map(AddrInfoHints::clone),
    );
    ctx.cmd_tx.send(req)?;
    match ctx.cmd_rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
        Ok(ResponseKind::GetAddrInfo(ai)) => Ok(ai),
        Err(e) => Err(e.into()),
        _ => panic!(""),
    }
}

pub struct CmId {
    pub(crate) ctx: Rc<Context>,
    pub(crate) handle: interface::CmId,
    pub(crate) qp: verbs::QueuePair,
}

unsafe impl Send for CmId {}
unsafe impl Sync for CmId {}

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
    /// Creates an identifier that is used to track communication information.
    pub fn create_ep(
        ctx: Rc<Context>,
        ai: &AddrInfo,
        pd: Option<&verbs::ProtectionDomain>,
        qp_init_attr: Option<&verbs::QpInitAttr>,
    ) -> Result<Self, Error> {
        let req = Request::CreateEp(
            ai.clone(),
            pd.map(|pd| pd.inner.0),
            qp_init_attr.map(|attr| interface::QpInitAttr::from_borrow(attr)),
        );
        ctx.cmd_tx.send(req)?;

        match ctx.cmd_rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
            Ok(ResponseKind::CreateEp(cmid)) => Ok(CmId {
                ctx,
                handle: cmid.handle,
                qp: verbs::QueuePair::from(cmid.qp),
            }),
            Err(e) => Err(e.into()),
            _ => panic!(""),
        }
    }

    pub fn listen(&self, backlog: i32) -> Result<(), Error> {
        let req = Request::Listen(self.handle.0, backlog);
        self.ctx.cmd_tx.send(req)?;
        rx_recv_impl!(self.ctx.cmd_rx, ResponseKind::Listen, { Ok(()) })
    }

    pub fn get_requst(&self) -> Result<CmId, Error> {
        let req = Request::GetRequest(self.handle.0);
        self.ctx.cmd_tx.send(req)?;

        match self
            .ctx
            .cmd_rx
            .recv()
            .map_err(|e| Error::IpcRecvError(e))?
            .0
        {
            Ok(ResponseKind::GetRequest(cmid)) => Ok(CmId {
                ctx: Rc::clone(&self.ctx),
                handle: cmid.handle,
                qp: verbs::QueuePair::from(cmid.qp),
            }),
            Err(e) => Err(e.into()),
            _ => panic!(""),
        }
    }

    pub fn accept(&self, conn_param: Option<&verbs::ConnParam>) -> Result<(), Error> {
        let req = Request::Accept(
            self.handle.0,
            conn_param.map(|param| interface::ConnParam::from_borrow(param)),
        );
        self.ctx.cmd_tx.send(req)?;
        rx_recv_impl!(self.ctx.cmd_rx, ResponseKind::Accept, { Ok(()) })
    }

    pub fn connect(&self, conn_param: Option<&verbs::ConnParam>) -> Result<(), Error> {
        let req = Request::Connect(
            self.handle.0,
            conn_param.map(|param| interface::ConnParam::from_borrow(param)),
        );
        self.ctx.cmd_tx.send(req)?;
        rx_recv_impl!(self.ctx.cmd_rx, ResponseKind::Connect, { Ok(()) })
    }

    pub fn reg_msgs<T>(&self, buffer: &[T]) -> Result<verbs::MemoryRegion, Error> {
        use nix::sys::mman::{mmap, MapFlags, ProtFlags};
        use std::slice;

        // 1. send regmsgs request to koala server
        let req = Request::RegMsgs(self.handle.0, slice_to_range(buffer));
        self.ctx.cmd_tx.send(req)?;
        // 2. receive file descriptors of the shared memories
        let fd = ipc::recv_fd(&self.ctx.sock)?;
        let mut memfd = unsafe { File::from_raw_fd(fd) };
        let shm_len = memfd.metadata()?.len() as usize;

        // 3. because the received are all new pages (pages haven't been mapped in the client's address
        //    space), copy the content of the original pages to the shared memory
        let addr = buffer.as_ptr() as usize;
        let len = buffer.len();

        let page_size = 4096;
        let aligned_end = (addr + len + page_size - 1) / page_size * page_size;
        let aligned_begin = addr - addr % page_size;
        let aligned_len = aligned_end - aligned_begin;
        assert_eq!(aligned_len, shm_len);

        let page_aligned_mem =
            unsafe { slice::from_raw_parts(aligned_begin as *const u8, aligned_len) };
        use std::io::Write;
        memfd.write_all(page_aligned_mem)?;

        // 4. mmap the shared memory in place (MAP_FIXED) to replace the client's original memory
        let pa = unsafe {
            mmap(
                aligned_begin as *mut libc::c_void,
                aligned_len,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED | MapFlags::MAP_NORESERVE | MapFlags::MAP_FIXED,
                memfd.as_raw_fd(),
                0,
            )
            .map_err(io::Error::from)?
        };

        assert_eq!(pa, aligned_begin as _);

        match self
            .ctx
            .cmd_rx
            .recv()
            .map_err(|e| Error::IpcRecvError(e))?
            .0
        {
            Ok(ResponseKind::RegMsgs(handle)) => Ok(verbs::MemoryRegion::new(handle, memfd)),
            Err(e) => Err(e.into()),
            _ => panic!(""),
        }
    }
}
