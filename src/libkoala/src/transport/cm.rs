use std::any::Any;
use std::mem;
use std::net::ToSocketAddrs;

use ipc::transport::rdma::cmd::{Command, CompletionKind};

use crate::transport::Error;
use crate::{rx_recv_impl, FromBorrow};
use crate::transport::KL_CTX;
use crate::transport::verbs;
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
    let req = Command::GetAddrInfo(
        node.map(String::from),
        service.map(String::from),
        hints.map(AddrInfoHints::clone),
    );

    KL_CTX.with(|ctx| {
        ctx.service.send_cmd(req)?;
        match ctx.service.recv_comp()?.0 {
            Ok(CompletionKind::GetAddrInfo(ai)) => Ok(ai),
            Err(e) => Err(Error::Interface("getaddrinfo", e)),
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

impl<'pd, 'ctx, 'scq, 'rcq, 'srq> Default for CmIdBuilder<'pd, 'ctx, 'scq, 'rcq, 'srq> {
    fn default() -> Self {
        Self::new()
    }
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
            let req = Command::CreateId(PortSpace::TCP);
            ctx.service.send_cmd(req)?;
            let cmid = rx_recv_impl!(ctx.service, CompletionKind::CreateId, cmid, { Ok(cmid) })?;
            assert!(cmid.qp.is_none());
            // auto drop if any of the following step failed
            let drop_cmid = DropCmId(cmid.handle);
            // bind_addr
            let req = Command::BindAddr(cmid.handle.0, listen_addr);
            ctx.service.send_cmd(req)?;
            rx_recv_impl!(ctx.service, CompletionKind::BindAddr)?;
            // listen
            let req = Command::Listen(cmid.handle.0, 512);
            ctx.service.send_cmd(req)?;
            rx_recv_impl!(ctx.service, CompletionKind::Listen)?;
            mem::forget(drop_cmid);
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
            let req = Command::CreateId(PortSpace::TCP);
            ctx.service.send_cmd(req)?;
            let cmid = rx_recv_impl!(ctx.service, CompletionKind::CreateId, cmid, { Ok(cmid) })?;
            assert!(cmid.qp.is_none());
            // auto drop if any of the following step failed
            let drop_cmid = DropCmId(cmid.handle);
            // resolve_addr
            let req = Command::ResolveAddr(cmid.handle.0, connect_addr);
            ctx.service.send_cmd(req)?;
            rx_recv_impl!(ctx.service, CompletionKind::ResolveAddr)?;
            // resolve_route
            let req = Command::ResolveRoute(cmid.handle.0, 2000);
            ctx.service.send_cmd(req)?;
            rx_recv_impl!(ctx.service, CompletionKind::ResolveRoute)?;
            assert!(cmid.qp.is_none());
            let mut builder = self.clone();
            builder.handle = cmid.handle;
            mem::forget(drop_cmid);
            Ok(builder)
        })
    }

    pub fn build(&self) -> Result<PreparedCmId, Error> {
        KL_CTX.with(|ctx| {
            // create_qp
            let pd = self.pd.map(|pd| pd.inner);
            let req = Command::CmCreateQp(
                self.handle.0,
                pd,
                interface::QpInitAttr::from_borrow(&self.qp_init_attr),
            );
            ctx.service.send_cmd(req)?;
            let qp = rx_recv_impl!(ctx.service, CompletionKind::CmCreateQp, qp, { Ok(qp) })?;
            Ok(PreparedCmId {
                inner: Inner {
                    handle: self.handle,
                    qp: verbs::QueuePair::open(qp)?,
                },
            })
        })
    }
}

struct DropCmId(interface::CmId);

impl Drop for DropCmId {
    fn drop(&mut self) {
        (|| {
            KL_CTX.with(|ctx| {
                let req = Command::DestroyId(self.0);
                ctx.service.send_cmd(req)?;
                rx_recv_impl!(ctx.service, CompletionKind::DestroyId)?;
                Ok(())
            })
        })()
        .unwrap_or_else(|e: Error| eprintln!("Destroying CmId: {}", e));
    }
}

pub struct CmIdListener<'pd, 'ctx, 'scq, 'rcq, 'srq> {
    pub(crate) handle: interface::CmId,
    builder: CmIdBuilder<'pd, 'ctx, 'scq, 'rcq, 'srq>,
}

impl<'pd, 'ctx, 'scq, 'rcq, 'srq> Drop for CmIdListener<'pd, 'ctx, 'scq, 'rcq, 'srq> {
    fn drop(&mut self) {
        let _drop_cmid = DropCmId(self.handle);
    }
}

impl<'pd, 'ctx, 'scq, 'rcq, 'srq> CmIdListener<'pd, 'ctx, 'scq, 'rcq, 'srq> {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> Result<Self, Error> {
        CmIdBuilder::new().bind(addr)
    }

    pub fn get_request(&self) -> Result<CmIdBuilder<'pd, 'ctx, 'scq, 'rcq, 'srq>, Error> {
        KL_CTX.with(|ctx| {
            let req = Command::GetRequest(self.handle.0);
            ctx.service.send_cmd(req)?;
            let cmid = rx_recv_impl!(ctx.service, CompletionKind::GetRequest, cmid, { Ok(cmid) })?;
            assert!(cmid.qp.is_none());
            let mut builder = self.builder.clone();
            builder.handle = cmid.handle;
            Ok(builder)
        })
    }

    pub fn try_get_request(&self) -> Result<Option<CmIdBuilder<'pd, 'ctx, 'scq, 'rcq, 'srq>>, Error> {
        KL_CTX.with(|ctx| {
            let req = Command::TryGetRequest(self.handle.0);
            ctx.service.send_cmd(req)?;
            let maybe_cmid = rx_recv_impl!(ctx.service, CompletionKind::TryGetRequest, cmid, { Ok(cmid) })?;
            if let Some(cmid) = maybe_cmid.as_ref() {
                assert!(cmid.qp.is_none());
                let mut builder = self.builder.clone();
                builder.handle = cmid.handle;
                Ok(Some(builder))
            } else {
                Ok(None)
            }
        })
    }
}

#[derive(Debug)]
pub struct PreparedCmId {
    pub(crate) inner: Inner,
}

impl PreparedCmId {
    pub fn accept(self, conn_param: Option<&ConnParam>) -> Result<CmId, Error> {
        KL_CTX.with(|ctx| {
            // accept
            let req = Command::Accept(
                self.inner.handle.0,
                conn_param.map(|param| interface::ConnParam::from_borrow(&param)),
            );
            ctx.service.send_cmd(req)?;
            rx_recv_impl!(ctx.service, CompletionKind::Accept)?;
            Ok(CmId { inner: self.inner })
        })
    }

    pub fn connect(self, conn_param: Option<&ConnParam>) -> Result<CmId, Error> {
        KL_CTX.with(|ctx| {
            // connect
            let req = Command::Connect(
                self.inner.handle.0,
                conn_param.map(|param| interface::ConnParam::from_borrow(&param)),
            );
            ctx.service.send_cmd(req)?;
            rx_recv_impl!(
                ctx.service,
                CompletionKind::Connect,
                { Ok(CmId { inner: self.inner }) },
                e,
                { Err(Error::Connect(e)) }
            )
        })
    }
}

#[derive(Debug)]
pub struct CmId {
    pub(crate) inner: Inner,
}

impl Drop for CmId {
    fn drop(&mut self) {
        (|| {
            KL_CTX.with(|ctx| {
                let req = Command::Disconnect(self.inner.handle);
                ctx.service.send_cmd(req)?;
                rx_recv_impl!(ctx.service, CompletionKind::Disconnect)?;
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
}

macro_rules! impl_for_cmid {
    ($type:ty, $member:ident) => {
        impl $type {
            pub fn qp(&self) -> &verbs::QueuePair {
                &self.$member.qp
            }

            pub fn alloc_msgs<T: Sized + Copy>(
                &self,
                len: usize,
            ) -> Result<verbs::MemoryRegion<T>, Error> {
                self.$member.alloc_msgs(len)
            }

            pub fn alloc_write<T: Sized + Copy>(
                &self,
                len: usize,
            ) -> Result<verbs::MemoryRegion<T>, Error> {
                self.$member.alloc_write(len)
            }

            pub fn alloc_read<T: Sized + Copy>(
                &self,
                len: usize,
            ) -> Result<verbs::MemoryRegion<T>, Error> {
                self.$member.alloc_read(len)
            }
        }
    };
}

impl_for_cmid!(CmId, inner);
impl_for_cmid!(PreparedCmId, inner);

#[derive(Debug)]
pub(crate) struct Inner {
    pub(crate) handle: interface::CmId,
    pub(crate) qp: verbs::QueuePair,
}

unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}

impl Drop for Inner {
    fn drop(&mut self) {
        let _drop_cmid = DropCmId(self.handle);
    }
}

impl Inner {
    // /// Creates an identifier that is used to track communication information.
    // pub fn create_ep(
    //     ai: &AddrInfo,
    //     pd: Option<&verbs::ProtectionDomain>,
    //     qp_init_attr: Option<&verbs::QpInitAttr>,
    // ) -> Result<Self, Error> {
    //     let req = Command::CreateEp(
    //         ai.clone(),
    //         pd.map(|pd| pd.inner),
    //         qp_init_attr.map(|attr| interface::QpInitAttr::from_borrow(attr)),
    //     );
    //     KL_CTX.with(|ctx| {
    //         ctx.service.send_cmd(req)?;

    //         match ctx.cmd_rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
    //             Ok(CompletionKind::CreateEp(cmid)) => Ok(CmId {
    //                 handle: cmid.handle,
    //                 qp: cmid.qp.map(|qp| verbs::QueuePair::open(qp)).transpose()?.unwrap(),
    //             }),
    //             Err(e) => Err(e.into()),
    //             _ => panic!(""),
    //         }
    //     })
    // }

    // pub fn listen(&self, backlog: i32) -> Result<(), Error> {
    //     let req = Command::Listen(self.handle.0, backlog);
    //     KL_CTX.with(|ctx| {
    //         ctx.service.send_cmd(req)?;
    //         rx_recv_impl!(ctx.cmd_rx, CompletionKind::Listen, { Ok(()) })
    //     })
    // }

    // pub fn get_request(&self) -> Result<CmId, Error> {
    //     let req = Command::GetRequest(self.handle.0);
    //     KL_CTX.with(|ctx| {
    //         ctx.service.send_cmd(req)?;

    //         match ctx.cmd_rx.recv().map_err(|e| Error::IpcRecvError(e))?.0 {
    //             Ok(CompletionKind::GetRequest(cmid)) => Ok(CmId {
    //                 handle: cmid.handle,
    //                 qp: cmid.qp.map(|qp| verbs::QueuePair::open(qp)).transpose()?,
    //             }),
    //             Err(e) => Err(e.into()),
    //             _ => panic!(""),
    //         }
    //     })
    // }

    // pub fn accept(&self, conn_param: Option<&verbs::ConnParam>) -> Result<(), Error> {
    //     let req = Command::Accept(
    //         self.handle.0,
    //         conn_param.map(|param| interface::ConnParam::from_borrow(param)),
    //     );
    //     KL_CTX.with(|ctx| {
    //         ctx.service.send_cmd(req)?;
    //         rx_recv_impl!(ctx.cmd_rx, CompletionKind::Accept, { Ok(()) })
    //     })
    // }

    // pub fn connect(&self, conn_param: Option<&verbs::ConnParam>) -> Result<(), Error> {
    //     let req = Command::Connect(
    //         self.handle.0,
    //         conn_param.map(|param| interface::ConnParam::from_borrow(param)),
    //     );
    //     KL_CTX.with(|ctx| {
    //         ctx.service.send_cmd(req)?;
    //         rx_recv_impl!(ctx.cmd_rx, CompletionKind::Connect, { Ok(()) })
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
