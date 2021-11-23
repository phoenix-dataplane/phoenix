use std::fs::File;
use std::io;
use std::os::unix::io::{AsRawFd, FromRawFd};

use ipc::buf;
use ipc::cmd::{Request, ResponseKind};

use crate::{verbs, Error, FromBorrow, KL_CTX};

// Re-exports
pub use interface::addrinfo::{AddrFamily, AddrInfo, AddrInfoFlags, AddrInfoHints, PortSpace};

/// Address and route resolution service.
pub fn getaddrinfo(
    node: Option<&str>,
    service: Option<&str>,
    hints: Option<&AddrInfoHints>,
) -> Result<AddrInfo, Error> {
    let req = Request::GetAddrInfo(
        node.map(String::from),
        service.map(String::from),
        hints.map(AddrInfoHints::clone),
    );

    KL_CTX.with(|ctx| {
        ctx.cmd_tx.send(req)?;
        match ctx.cmd_rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
            Ok(ResponseKind::GetAddrInfo(ai)) => Ok(ai),
            Err(e) => Err(e.into()),
            _ => panic!(""),
        }
    })
}

macro_rules! rx_recv_impl {
    ($rx:expr, $resp:path, $inst:ident, $ok_block:block) => {
        match $rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
            Ok($resp($inst)) => $ok_block,
            Err(e) => Err(Error::from(e)),
            _ => panic!(""),
        }
    };
    ($rx:expr, $resp:path, $ok_block:block) => {
        match $rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
            Ok($resp) => $ok_block,
            Err(e) => Err(Error::from(e)),
            _ => panic!(""),
        }
    };
    ($rx:expr, $resp:path) => {
        match $rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
            Ok($resp) => Ok(()),
            Err(e) => Err(Error::from(e)),
            _ => panic!(""),
        }
    };
}

pub struct CmId {
    pub(crate) handle: interface::CmId,
    // it could be empty for listener QP.
    pub qp: Option<verbs::QueuePair>,
}

unsafe impl Send for CmId {}
unsafe impl Sync for CmId {}

impl Drop for CmId {
    fn drop(&mut self) {
        if self.qp.is_some() {
            // for listener CmId, no need to call disconnect
            (|| {
                KL_CTX.with(|ctx| {
                    let disconnect_req = Request::Disconnect(self.handle);
                    ctx.cmd_tx.send(disconnect_req)?;
                    rx_recv_impl!(ctx.cmd_rx, ResponseKind::Disconnect)?;
                    Ok(())
                })
            })()
            .unwrap_or_else(|e: Error| eprintln!("Disconnecting and dropping CmId: {}", e));
        }
        (|| {
            KL_CTX.with(|ctx| {
                let destroy_req = Request::DestroyId(self.handle);
                ctx.cmd_tx.send(destroy_req)?;
                rx_recv_impl!(ctx.cmd_rx, ResponseKind::DestroyId)?;
                Ok(())
            })
        })()
        .unwrap_or_else(|e: Error| eprintln!("Disconnecting and dropping CmId: {}", e));
    }
}

impl CmId {
    /// Creates an identifier that is used to track communication information.
    pub fn create_ep(
        ai: &AddrInfo,
        pd: Option<&verbs::ProtectionDomain>,
        qp_init_attr: Option<&verbs::QpInitAttr>,
    ) -> Result<Self, Error> {
        let req = Request::CreateEp(
            ai.clone(),
            pd.map(|pd| pd.inner.0),
            qp_init_attr.map(|attr| interface::QpInitAttr::from_borrow(attr)),
        );
        KL_CTX.with(|ctx| {
            ctx.cmd_tx.send(req)?;

            match ctx.cmd_rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
                Ok(ResponseKind::CreateEp(cmid)) => Ok(CmId {
                    handle: cmid.handle,
                    qp: cmid.qp.map(|qp| verbs::QueuePair::from(qp)),
                }),
                Err(e) => Err(e.into()),
                _ => panic!(""),
            }
        })
    }

    pub fn listen(&self, backlog: i32) -> Result<(), Error> {
        let req = Request::Listen(self.handle.0, backlog);
        KL_CTX.with(|ctx| {
            ctx.cmd_tx.send(req)?;
            rx_recv_impl!(ctx.cmd_rx, ResponseKind::Listen, { Ok(()) })
        })
    }

    pub fn get_request(&self) -> Result<CmId, Error> {
        let req = Request::GetRequest(self.handle.0);
        KL_CTX.with(|ctx| {
            ctx.cmd_tx.send(req)?;

            match ctx.cmd_rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
                Ok(ResponseKind::GetRequest(cmid)) => Ok(CmId {
                    handle: cmid.handle,
                    qp: cmid.qp.map(|qp| verbs::QueuePair::from(qp)),
                }),
                Err(e) => Err(e.into()),
                _ => panic!(""),
            }
        })
    }

    pub fn accept(&self, conn_param: Option<&verbs::ConnParam>) -> Result<(), Error> {
        let req = Request::Accept(
            self.handle.0,
            conn_param.map(|param| interface::ConnParam::from_borrow(param)),
        );
        KL_CTX.with(|ctx| {
            ctx.cmd_tx.send(req)?;
            rx_recv_impl!(ctx.cmd_rx, ResponseKind::Accept, { Ok(()) })
        })
    }

    pub fn connect(&self, conn_param: Option<&verbs::ConnParam>) -> Result<(), Error> {
        let req = Request::Connect(
            self.handle.0,
            conn_param.map(|param| interface::ConnParam::from_borrow(param)),
        );
        KL_CTX.with(|ctx| {
            ctx.cmd_tx.send(req)?;
            rx_recv_impl!(ctx.cmd_rx, ResponseKind::Connect, { Ok(()) })
        })
    }

    pub fn reg_msgs<T>(&self, buffer: &[T]) -> Result<verbs::MemoryRegion, Error> {
        use nix::sys::mman::{mmap, MapFlags, ProtFlags};
        use std::slice;

        // 1. send regmsgs request to koala server
        let buffer = buf::Buffer::from(buffer);
        let req = Request::RegMsgs(self.handle.0, buffer);
        KL_CTX.with(|ctx| ctx.cmd_tx.send(req))?;
        // 2. receive file descriptors of the shared memories
        let fds = KL_CTX.with(|ctx| ipc::recv_fd(&ctx.sock))?;
        assert_eq!(fds.len(), 1);
        let mut memfd = unsafe { File::from_raw_fd(fds[0]) };
        let shm_len = memfd.metadata()?.len() as usize;

        // 3. because the received are all new pages (pages haven't been mapped in the client's address
        //    space), copy the content of the original pages to the shared memory
        let addr = buffer.addr as usize;
        let len = buffer.len as usize;

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

        KL_CTX.with(
            |ctx| match ctx.cmd_rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
                Ok(ResponseKind::RegMsgs(handle)) => Ok(verbs::MemoryRegion::new(handle, memfd)),
                Err(e) => Err(e.into()),
                _ => panic!(""),
            },
        )
    }
}
