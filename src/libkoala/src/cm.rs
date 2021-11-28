use ipc::cmd::{Request, ResponseKind};

use crate::{rx_recv_impl, verbs, Error, FromBorrow, KL_CTX};
use verbs::AccessFlags;

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
                    let req = Request::Disconnect(self.handle);
                    ctx.cmd_tx.send(req)?;
                    rx_recv_impl!(ctx.cmd_rx, ResponseKind::Disconnect)?;
                    Ok(())
                })
            })()
            .unwrap_or_else(|e: Error| eprintln!("Disconnecting and dropping CmId: {}", e));
        }
        (|| {
            KL_CTX.with(|ctx| {
                let req = Request::DestroyId(self.handle);
                ctx.cmd_tx.send(req)?;
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
                    qp: cmid.qp.map(|qp| verbs::QueuePair::open(qp)).transpose()?,
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
                    qp: cmid.qp.map(|qp| verbs::QueuePair::open(qp)).transpose()?,
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

    pub fn alloc_msgs<T: Sized + Copy>(&self, len: usize) -> Result<verbs::MemoryRegion<T>, Error> {
        self.qp
            .as_ref()
            .unwrap()
            .pd
            .allocate(len, AccessFlags::LOCAL_WRITE)
    }

    pub fn alloc_write<T: Sized + Copy>(
        &self,
        len: usize,
    ) -> Result<verbs::MemoryRegion<T>, Error> {
        self.qp
            .as_ref()
            .unwrap()
            .pd
            .allocate(len, AccessFlags::LOCAL_WRITE | AccessFlags::REMOTE_WRITE)
    }

    pub fn alloc_read<T: Sized + Copy>(&self, len: usize) -> Result<verbs::MemoryRegion<T>, Error> {
        self.qp
            .as_ref()
            .unwrap()
            .pd
            .allocate(len, AccessFlags::LOCAL_WRITE | AccessFlags::REMOTE_READ)
    }
}
