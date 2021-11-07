use std::fs::File;
use std::io;
use std::os::unix::io::{AsRawFd, FromRawFd};

use interface::{
    addrinfo::{AddrInfo, AddrInfoHints},
    CmId, ConnParam, MemoryRegion, ProtectionDomain, QpInitAttr,
};
use ipc::cmd::{Request, ResponseKind};
use ipc::interface::{ConnParamOwned, FromBorrow, QpInitAttrOwned};

use crate::{slice_to_range, Context, Error};

/// Creates an identifier that is used to track communication information.
pub fn create_ep(
    ctx: &Context,
    ai: &AddrInfo,
    pd: Option<&ProtectionDomain>,
    qp_init_attr: Option<&QpInitAttr>,
) -> Result<CmId, Error> {
    let req = Request::CreateEp(
        ai.clone(),
        pd.map(|pd| pd.0),
        qp_init_attr.map(|attr| QpInitAttrOwned::from_borrow(attr)),
    );
    ctx.cmd_tx.send(req)?;

    match ctx.cmd_rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
        Ok(ResponseKind::CreateEp(handle)) => Ok(CmId(handle)),
        Err(e) => Err(e.into()),
        _ => panic!(""),
    }
}

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

pub fn listen(ctx: &Context, id: &CmId, backlog: i32) -> Result<(), Error> {
    let req = Request::Listen(id.0, backlog);
    ctx.cmd_tx.send(req)?;
    rx_recv_impl!(ctx.cmd_rx, ResponseKind::Listen, { Ok(()) })
}

pub fn get_requst(ctx: &Context, listen: &CmId) -> Result<CmId, Error> {
    let req = Request::GetRequest(listen.0);
    ctx.cmd_tx.send(req)?;
    rx_recv_impl!(ctx.cmd_rx, ResponseKind::GetRequest, handle, {
        Ok(CmId(handle))
    })
}

pub fn accept(ctx: &Context, id: &CmId, conn_param: Option<&ConnParam>) -> Result<(), Error> {
    let req = Request::Accept(
        id.0,
        conn_param.map(|param| ConnParamOwned::from_borrow(param)),
    );
    ctx.cmd_tx.send(req)?;
    rx_recv_impl!(ctx.cmd_rx, ResponseKind::Accept, { Ok(()) })
}

pub fn connect(ctx: &Context, id: &CmId, conn_param: Option<&ConnParam>) -> Result<(), Error> {
    let req = Request::Connect(
        id.0,
        conn_param.map(|param| ConnParamOwned::from_borrow(param)),
    );
    ctx.cmd_tx.send(req)?;
    rx_recv_impl!(ctx.cmd_rx, ResponseKind::Connect, { Ok(()) })
}

pub fn reg_msgs<T>(ctx: &Context, id: &CmId, buffer: &[T]) -> Result<MemoryRegion, Error> {
    use nix::sys::mman::{mmap, MapFlags, ProtFlags};
    use std::slice;

    // 1. send regmsgs request to koala server
    let req = Request::RegMsgs(id.0, slice_to_range(buffer));
    ctx.cmd_tx.send(req)?;
    // 2. receive file descriptors of the shared memories
    let fd = ipc::recv_fd(&ctx.sock)?;
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

    match ctx.cmd_rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
        Ok(ResponseKind::RegMsgs(handle)) => Ok(MemoryRegion::new(handle, memfd)),
        Err(e) => Err(e.into()),
        _ => panic!(""),
    }
}
