use std::any::Any;
use std::mem;
use std::net::ToSocketAddrs;

use interface::{AsHandle, Handle};
use ipc::transport::rdma::cmd::{Command, CompletionKind};

use super::{get_service, rx_recv_impl, uverbs, Error, FromBorrow};
use uverbs::AccessFlags;
use uverbs::{ConnParam, ProtectionDomain, QpInitAttr};

// Re-exports
pub use interface::addrinfo::{AddrFamily, AddrInfo, AddrInfoFlags, AddrInfoHints, PortSpace};

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

    pub fn set_send_cq(&mut self, send_cq: &'scq uverbs::CompletionQueue) -> &mut Self {
        self.qp_init_attr.send_cq = Some(send_cq);
        self
    }

    pub fn set_recv_cq(&mut self, recv_cq: &'rcq uverbs::CompletionQueue) -> &mut Self {
        self.qp_init_attr.recv_cq = Some(recv_cq);
        self
    }

    pub fn set_cap(&mut self, cap: uverbs::QpCapability) -> &mut Self {
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

    pub fn bind<A: ToSocketAddrs>(&self, addr: A) -> Result<CmIdListener, Error> {
        let listen_addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or(Error::NoAddrResolved)?;
        let service = get_service();
        // create_id
        let req = Command::CreateId(PortSpace::TCP);
        service.send_cmd(req)?;
        let cmid = rx_recv_impl!(service, CompletionKind::CreateId, cmid, { Ok(cmid) })?;
        assert!(cmid.qp.is_none());
        // auto drop if any of the following step failed
        let drop_cmid = DropCmId(cmid.handle);
        // bind_addr
        let req = Command::BindAddr(cmid.handle.0, listen_addr);
        service.send_cmd(req)?;
        rx_recv_impl!(service, CompletionKind::BindAddr)?;
        // listen
        let req = Command::Listen(cmid.handle.0, 512);
        service.send_cmd(req)?;
        rx_recv_impl!(service, CompletionKind::Listen)?;
        mem::forget(drop_cmid);
        Ok(CmIdListener {
            handle: cmid.handle,
        })
    }

    pub fn resolve_route<A: ToSocketAddrs>(&self, addr: A) -> Result<Self, Error> {
        let connect_addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or(Error::NoAddrResolved)?;
        let service = get_service();
        // create_id
        let req = Command::CreateId(PortSpace::TCP);
        service.send_cmd(req)?;
        let cmid = rx_recv_impl!(service, CompletionKind::CreateId, cmid, { Ok(cmid) })?;
        assert!(cmid.qp.is_none());
        // auto drop if any of the following step failed
        let drop_cmid = DropCmId(cmid.handle);
        // resolve_addr
        let req = Command::ResolveAddr(cmid.handle.0, connect_addr);
        service.send_cmd(req)?;
        rx_recv_impl!(service, CompletionKind::ResolveAddr)?;
        // resolve_route
        let req = Command::ResolveRoute(cmid.handle.0, 2000);
        service.send_cmd(req)?;
        rx_recv_impl!(service, CompletionKind::ResolveRoute)?;
        assert!(cmid.qp.is_none());
        let mut builder = self.clone();
        builder.handle = cmid.handle;
        mem::forget(drop_cmid);
        Ok(builder)
    }

    pub fn build(&self) -> Result<PreparedCmId, Error> {
        let service = get_service();
        // create_qp
        let pd = self.pd.map(|pd| pd.inner);
        let req = Command::CmCreateQp(
            self.handle.0,
            pd,
            interface::QpInitAttr::from_borrow(&self.qp_init_attr),
        );
        service.send_cmd(req)?;
        let qp = rx_recv_impl!(service, CompletionKind::CmCreateQp, qp, { Ok(qp) })?;
        Ok(PreparedCmId {
            inner: Inner {
                handle: self.handle,
                qp: uverbs::QueuePair::open(qp)?,
            },
        })
    }
}

struct DropCmId(interface::CmId);

impl Drop for DropCmId {
    fn drop(&mut self) {
        (|| {
            let service = get_service();
            let req = Command::DestroyId(self.0);
            service.send_cmd(req)?;
            rx_recv_impl!(service, CompletionKind::DestroyId)?;
            Ok(())
        })()
        .unwrap_or_else(|e: Error| eprintln!("Destroying CmId: {}", e));
    }
}

// COMMENT(cjr): here we remove some functionalities of the CmIdListener from the libkoala so that
// the 5 lifetime generices does not need to propagate.
pub struct CmIdListener {
    pub(crate) handle: interface::CmId,
}

impl AsHandle for CmIdListener {
    #[inline]
    fn as_handle(&self) -> Handle {
        self.handle.0
    }
}

impl Drop for CmIdListener {
    fn drop(&mut self) {
        let _drop_cmid = DropCmId(self.handle);
    }
}

impl CmIdListener {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> Result<Self, Error> {
        CmIdBuilder::new().bind(addr)
    }

    pub fn get_request<'pd, 'ctx, 'scq, 'rcq, 'srq>(
        &self,
    ) -> Result<CmIdBuilder<'pd, 'ctx, 'scq, 'rcq, 'srq>, Error> {
        let service = get_service();
        let req = Command::GetRequest(self.handle.0);
        service.send_cmd(req)?;
        let cmid = rx_recv_impl!(service, CompletionKind::GetRequest, cmid, { Ok(cmid) })?;
        assert!(cmid.qp.is_none());
        let mut builder = CmIdBuilder::new();
        builder.handle = cmid.handle;
        Ok(builder)
    }
}

#[derive(Debug)]
pub struct PreparedCmId {
    pub(crate) inner: Inner,
}

impl AsHandle for PreparedCmId {
    #[inline]
    fn as_handle(&self) -> Handle {
        self.inner.handle.0
    }
}

impl PreparedCmId {
    pub fn accept(self, conn_param: Option<&ConnParam>) -> Result<CmId, Error> {
        let service = get_service();
        // accept
        let req = Command::Accept(
            self.inner.handle.0,
            conn_param.map(|param| interface::ConnParam::from_borrow(&param)),
        );
        service.send_cmd(req)?;
        rx_recv_impl!(service, CompletionKind::Accept)?;
        Ok(CmId { inner: self.inner })
    }

    pub fn connect(self, conn_param: Option<&ConnParam>) -> Result<CmId, Error> {
        let service = get_service();
        // connect
        let req = Command::Connect(
            self.inner.handle.0,
            conn_param.map(|param| interface::ConnParam::from_borrow(&param)),
        );
        service.send_cmd(req)?;
        rx_recv_impl!(
            service,
            CompletionKind::Connect,
            { Ok(CmId { inner: self.inner }) },
            e,
            { Err(Error::Connect(e)) }
        )
    }
}

#[derive(Debug)]
pub struct CmId {
    pub(crate) inner: Inner,
}

impl AsHandle for CmId {
    #[inline]
    fn as_handle(&self) -> Handle {
        self.inner.handle.0
    }
}

impl Drop for CmId {
    fn drop(&mut self) {
        (|| {
            let service = get_service();
            let req = Command::Disconnect(self.inner.handle);
            service.send_cmd(req)?;
            rx_recv_impl!(service, CompletionKind::Disconnect)?;

            Ok(())
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
            pub fn qp(&self) -> &uverbs::QueuePair {
                &self.$member.qp
            }

            pub fn alloc_msgs<T: Sized + Copy>(
                &self,
                len: usize,
            ) -> Result<uverbs::MemoryRegion<T>, Error> {
                self.$member.alloc_msgs(len)
            }

            pub fn alloc_write<T: Sized + Copy>(
                &self,
                len: usize,
            ) -> Result<uverbs::MemoryRegion<T>, Error> {
                self.$member.alloc_write(len)
            }

            pub fn alloc_read<T: Sized + Copy>(
                &self,
                len: usize,
            ) -> Result<uverbs::MemoryRegion<T>, Error> {
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
    pub(crate) qp: uverbs::QueuePair,
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
    pub fn alloc_msgs<T: Sized + Copy>(
        &self,
        len: usize,
    ) -> Result<uverbs::MemoryRegion<T>, Error> {
        self.qp.pd.allocate(len, AccessFlags::LOCAL_WRITE)
    }

    pub fn alloc_write<T: Sized + Copy>(
        &self,
        len: usize,
    ) -> Result<uverbs::MemoryRegion<T>, Error> {
        self.qp
            .pd
            .allocate(len, AccessFlags::LOCAL_WRITE | AccessFlags::REMOTE_WRITE)
    }

    pub fn alloc_read<T: Sized + Copy>(
        &self,
        len: usize,
    ) -> Result<uverbs::MemoryRegion<T>, Error> {
        self.qp
            .pd
            .allocate(len, AccessFlags::LOCAL_WRITE | AccessFlags::REMOTE_READ)
    }
}