use std::any::Any;
use std::net::ToSocketAddrs;

use ipc::cmd::{Request, ResponseKind};

use crate::{rx_recv_impl, verbs, Error, FromBorrow, KL_CTX};
use verbs::AccessFlags;
use verbs::{ConnParam, ProtectionDomain, QpInitAttr};

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

#[derive(Clone)]
pub struct CmIdBuilder<'pd, 'ctx, 'scq, 'rcq, 'srq> {
    handle: interface::CmId,
    pd: Option<&'pd ProtectionDomain>,
    qp_init_attr: QpInitAttr<'ctx, 'scq, 'rcq, 'srq>,
}

impl<'pd, 'ctx, 'scq, 'rcq, 'srq> CmIdBuilder<'pd, 'ctx, 'scq, 'rcq, 'srq> {
    pub fn new() -> Self {
        CmIdBuilder {
            handle: interface::CmId(interface::Handle::INVALID),
            pd: None,
            qp_init_attr: Default::default(),
        }
    }

    pub fn set_pd(&mut self, pd: &'pd ProtectionDomain) -> &mut Self {
        self.pd = Some(pd);
        self
    }

    pub fn set_qp_init_attr<'a>(
        &mut self,
        qp_init_attr: &'a QpInitAttr<'ctx, 'scq, 'rcq, 'srq>,
    ) -> &mut Self {
        self.qp_init_attr = qp_init_attr.clone();
        self
    }

    pub fn set_qp_context(&mut self, qp_context: &'ctx dyn Any) -> &mut Self {
        self.qp_init_attr.qp_context = Some(qp_context);
        self
    }

    pub fn set_send_cq(&mut self, send_cq: &'scq verbs::CompletionQueue) -> &mut Self {
        self.qp_init_attr.send_cq = Some(send_cq);
        self
    }

    pub fn set_recv_cq(&mut self, recv_cq: &'rcq verbs::CompletionQueue) -> &mut Self {
        self.qp_init_attr.recv_cq = Some(recv_cq);
        self
    }

    pub fn set_cap(&mut self, cap: verbs::QpCapability) -> &mut Self {
        self.qp_init_attr.cap = cap;
        self
    }

    pub fn set_max_send_wr(&mut self, max_send_wr: u32) -> &mut Self {
        self.qp_init_attr.cap.max_send_wr = max_send_wr;
        self
    }

    pub fn set_max_recv_wr(&mut self, max_recv_wr: u32) -> &mut Self {
        self.qp_init_attr.cap.max_recv_wr = max_recv_wr;
        self
    }

    pub fn set_max_send_sge(&mut self, max_send_sge: u32) -> &mut Self {
        self.qp_init_attr.cap.max_send_sge = max_send_sge;
        self
    }

    pub fn set_max_recv_sge(&mut self, max_recv_sge: u32) -> &mut Self {
        self.qp_init_attr.cap.max_recv_sge = max_recv_sge;
        self
    }

    pub fn set_max_inline_data(&mut self, max_inline_data: u32) -> &mut Self {
        self.qp_init_attr.cap.max_inline_data = max_inline_data;
        self
    }

    pub fn signal_all(&mut self) -> &mut Self {
        self.qp_init_attr.sq_sig_all = true;
        self
    }

    pub fn bind<A: ToSocketAddrs>(
        &self,
        addr: A,
    ) -> Result<CmIdListener<'pd, 'ctx, 'scq, 'rcq, 'srq>, Error> {
        let listen_addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or(Error::NoAddrResolved)?;
        KL_CTX.with(|ctx| {
            // create_id
            let req = Request::CreateId(None, PortSpace::TCP);
            ctx.cmd_tx.send(req)?;
            let cmid = rx_recv_impl!(ctx.cmd_rx, ResponseKind::CreateId, cmid, { Ok(cmid) })?;
            assert!(cmid.qp.is_none());
            // bind_addr
            let req = Request::BindAddr(cmid.handle.0, listen_addr);
            ctx.cmd_tx.send(req)?;
            rx_recv_impl!(ctx.cmd_rx, ResponseKind::BindAddr)?;
            // listen
            let req = Request::Listen(cmid.handle.0, 512);
            ctx.cmd_tx.send(req)?;
            rx_recv_impl!(ctx.cmd_rx, ResponseKind::Listen)?;
            Ok(CmIdListener {
                handle: cmid.handle,
                builder: self.clone(),
            })
        })
    }

    pub fn resolve_route<A: ToSocketAddrs>(&self, addr: A) -> Result<Self, Error> {
        let connect_addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or(Error::NoAddrResolved)?;
        KL_CTX.with(|ctx| {
            // create_id
            let req = Request::CreateId(None, PortSpace::TCP);
            ctx.cmd_tx.send(req)?;
            let cmid = rx_recv_impl!(ctx.cmd_rx, ResponseKind::CreateId, cmid, { Ok(cmid) })?;
            assert!(cmid.qp.is_none());
            // resolve_addr
            let req = Request::ResolveAddr(cmid.handle.0, connect_addr);
            ctx.cmd_tx.send(req)?;
            rx_recv_impl!(ctx.cmd_rx, ResponseKind::ResolveAddr)?;
            // resolve_route
            let req = Request::ResolveRoute(cmid.handle.0, 2000);
            ctx.cmd_tx.send(req)?;
            rx_recv_impl!(ctx.cmd_rx, ResponseKind::ResolveRoute)?;
            assert!(cmid.qp.is_none());
            let mut builder = self.clone();
            builder.handle = cmid.handle;
            Ok(builder)
        })
    }

    pub fn build(&self) -> Result<PreparedCmId, Error> {
        KL_CTX.with(|ctx| {
            // create_qp
            let pd = self.pd.map(|pd| pd.inner.clone());
            let req = Request::CmCreateQp(
                self.handle.0,
                pd,
                interface::QpInitAttr::from_borrow(&self.qp_init_attr),
            );
            ctx.cmd_tx.send(req)?;
            let qp = rx_recv_impl!(ctx.cmd_rx, ResponseKind::CmCreateQp, qp, { Ok(qp) })?;
            Ok(PreparedCmId {
                inner: Inner {
                    handle: self.handle,
                    qp: verbs::QueuePair::open(qp)?,
                },
            })
        })
    }
}

pub struct CmIdListener<'pd, 'ctx, 'scq, 'rcq, 'srq> {
    pub(crate) handle: interface::CmId,
    builder: CmIdBuilder<'pd, 'ctx, 'scq, 'rcq, 'srq>,
}

impl<'pd, 'ctx, 'scq, 'rcq, 'srq> Drop for CmIdListener<'pd, 'ctx, 'scq, 'rcq, 'srq> {
    fn drop(&mut self) {
        (|| {
            KL_CTX.with(|ctx| {
                let req = Request::DestroyId(self.handle);
                ctx.cmd_tx.send(req)?;
                rx_recv_impl!(ctx.cmd_rx, ResponseKind::DestroyId)?;
                Ok(())
            })
        })()
        .unwrap_or_else(|e: Error| eprintln!("Destroying CmIdListener: {}", e));
    }
}

impl<'pd, 'ctx, 'scq, 'rcq, 'srq> CmIdListener<'pd, 'ctx, 'scq, 'rcq, 'srq> {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> Result<Self, Error> {
        CmIdBuilder::new().bind(addr)
    }

    pub fn get_request(&self) -> Result<CmIdBuilder<'pd, 'ctx, 'scq, 'rcq, 'srq>, Error> {
        KL_CTX.with(|ctx| {
            let req = Request::GetRequest(self.handle.0);
            ctx.cmd_tx.send(req)?;
            let cmid = rx_recv_impl!(ctx.cmd_rx, ResponseKind::GetRequest, cmid, { Ok(cmid) })?;
            assert!(cmid.qp.is_none());
            let mut builder = self.builder.clone();
            builder.handle = cmid.handle;
            Ok(builder)
        })
    }
}

pub struct PreparedCmId {
    pub(crate) inner: Inner,
}

impl PreparedCmId {
    pub fn accept(self, conn_param: Option<&ConnParam>) -> Result<CmId, Error> {
        KL_CTX.with(|ctx| {
            // accept
            let req = Request::Accept(
                self.inner.handle.0,
                conn_param.map(|param| interface::ConnParam::from_borrow(&param)),
            );
            ctx.cmd_tx.send(req)?;
            rx_recv_impl!(ctx.cmd_rx, ResponseKind::Accept)?;
            Ok(CmId { inner: self.inner })
        })
    }

    pub fn connect(self, conn_param: Option<&ConnParam>) -> Result<CmId, Error> {
        KL_CTX.with(|ctx| {
            // connect
            let req = Request::Connect(
                self.inner.handle.0,
                conn_param.map(|param| interface::ConnParam::from_borrow(&param)),
            );
            ctx.cmd_tx.send(req)?;
            rx_recv_impl!(ctx.cmd_rx, ResponseKind::Connect)?;
            Ok(CmId { inner: self.inner })
        })
    }

    pub fn qp(&self) -> &verbs::QueuePair {
        &self.inner.qp
    }

    pub fn alloc_msgs<T: Sized + Copy>(&self, len: usize) -> Result<verbs::MemoryRegion<T>, Error> {
        self.inner.alloc_msgs(len)
    }

    pub fn alloc_write<T: Sized + Copy>(
        &self,
        len: usize,
    ) -> Result<verbs::MemoryRegion<T>, Error> {
        self.inner.alloc_write(len)
    }

    pub fn alloc_read<T: Sized + Copy>(&self, len: usize) -> Result<verbs::MemoryRegion<T>, Error> {
        self.inner.alloc_read(len)
    }
}

pub struct CmId {
    pub(crate) inner: Inner,
}

impl Drop for CmId {
    fn drop(&mut self) {
        (|| {
            KL_CTX.with(|ctx| {
                let req = Request::Disconnect(self.inner.handle);
                ctx.cmd_tx.send(req)?;
                rx_recv_impl!(ctx.cmd_rx, ResponseKind::Disconnect)?;
                Ok(())
            })
        })()
        .unwrap_or_else(|e: Error| eprintln!("Disconnecting CmId: {}", e));
    }
}

impl CmId {
    pub fn resolve_route<'pd, 'ctx, 'scq, 'rcq, 'srq, A: ToSocketAddrs>(
        addr: A,
    ) -> Result<CmIdBuilder<'pd, 'ctx, 'scq, 'rcq, 'srq>, Error> {
        CmIdBuilder::new().resolve_route(addr)
    }

    pub fn qp(&self) -> &verbs::QueuePair {
        &self.inner.qp
    }

    pub fn alloc_msgs<T: Sized + Copy>(&self, len: usize) -> Result<verbs::MemoryRegion<T>, Error> {
        self.inner.alloc_msgs(len)
    }

    pub fn alloc_write<T: Sized + Copy>(
        &self,
        len: usize,
    ) -> Result<verbs::MemoryRegion<T>, Error> {
        self.inner.alloc_write(len)
    }

    pub fn alloc_read<T: Sized + Copy>(&self, len: usize) -> Result<verbs::MemoryRegion<T>, Error> {
        self.inner.alloc_read(len)
    }
}

pub(crate) struct Inner {
    pub(crate) handle: interface::CmId,
    pub(crate) qp: verbs::QueuePair,
}

unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

impl Drop for Inner {
    fn drop(&mut self) {
        (|| {
            KL_CTX.with(|ctx| {
                let req = Request::DestroyId(self.handle);
                ctx.cmd_tx.send(req)?;
                rx_recv_impl!(ctx.cmd_rx, ResponseKind::DestroyId)?;
                Ok(())
            })
        })()
        .unwrap_or_else(|e: Error| eprintln!("Destroying CmId: {}", e));
    }
}

impl Inner {
    // /// Creates an identifier that is used to track communication information.
    // pub fn create_ep(
    //     ai: &AddrInfo,
    //     pd: Option<&verbs::ProtectionDomain>,
    //     qp_init_attr: Option<&verbs::QpInitAttr>,
    // ) -> Result<Self, Error> {
    //     let req = Request::CreateEp(
    //         ai.clone(),
    //         pd.map(|pd| pd.inner),
    //         qp_init_attr.map(|attr| interface::QpInitAttr::from_borrow(attr)),
    //     );
    //     KL_CTX.with(|ctx| {
    //         ctx.cmd_tx.send(req)?;

    //         match ctx.cmd_rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
    //             Ok(ResponseKind::CreateEp(cmid)) => Ok(CmId {
    //                 handle: cmid.handle,
    //                 qp: cmid.qp.map(|qp| verbs::QueuePair::open(qp)).transpose()?.unwrap(),
    //             }),
    //             Err(e) => Err(e.into()),
    //             _ => panic!(""),
    //         }
    //     })
    // }

    // pub fn listen(&self, backlog: i32) -> Result<(), Error> {
    //     let req = Request::Listen(self.handle.0, backlog);
    //     KL_CTX.with(|ctx| {
    //         ctx.cmd_tx.send(req)?;
    //         rx_recv_impl!(ctx.cmd_rx, ResponseKind::Listen, { Ok(()) })
    //     })
    // }

    // pub fn get_request(&self) -> Result<CmId, Error> {
    //     let req = Request::GetRequest(self.handle.0);
    //     KL_CTX.with(|ctx| {
    //         ctx.cmd_tx.send(req)?;

    //         match ctx.cmd_rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
    //             Ok(ResponseKind::GetRequest(cmid)) => Ok(CmId {
    //                 handle: cmid.handle,
    //                 qp: cmid.qp.map(|qp| verbs::QueuePair::open(qp)).transpose()?,
    //             }),
    //             Err(e) => Err(e.into()),
    //             _ => panic!(""),
    //         }
    //     })
    // }

    // pub fn accept(&self, conn_param: Option<&verbs::ConnParam>) -> Result<(), Error> {
    //     let req = Request::Accept(
    //         self.handle.0,
    //         conn_param.map(|param| interface::ConnParam::from_borrow(param)),
    //     );
    //     KL_CTX.with(|ctx| {
    //         ctx.cmd_tx.send(req)?;
    //         rx_recv_impl!(ctx.cmd_rx, ResponseKind::Accept, { Ok(()) })
    //     })
    // }

    // pub fn connect(&self, conn_param: Option<&verbs::ConnParam>) -> Result<(), Error> {
    //     let req = Request::Connect(
    //         self.handle.0,
    //         conn_param.map(|param| interface::ConnParam::from_borrow(param)),
    //     );
    //     KL_CTX.with(|ctx| {
    //         ctx.cmd_tx.send(req)?;
    //         rx_recv_impl!(ctx.cmd_rx, ResponseKind::Connect, { Ok(()) })
    //     })
    // }

    pub fn alloc_msgs<T: Sized + Copy>(&self, len: usize) -> Result<verbs::MemoryRegion<T>, Error> {
        self.qp.pd.allocate(len, AccessFlags::LOCAL_WRITE)
    }

    pub fn alloc_write<T: Sized + Copy>(
        &self,
        len: usize,
    ) -> Result<verbs::MemoryRegion<T>, Error> {
        self.qp
            .pd
            .allocate(len, AccessFlags::LOCAL_WRITE | AccessFlags::REMOTE_WRITE)
    }

    pub fn alloc_read<T: Sized + Copy>(&self, len: usize) -> Result<verbs::MemoryRegion<T>, Error> {
        self.qp
            .pd
            .allocate(len, AccessFlags::LOCAL_WRITE | AccessFlags::REMOTE_READ)
    }
}
