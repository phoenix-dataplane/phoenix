use std::any::Any;
use std::mem;
use std::net::ToSocketAddrs;

use interface::{AsHandle, Handle};

use super::get_ops;
use super::{uverbs, Error, FromBorrow};
use uverbs::AccessFlags;
use uverbs::{ConnParam, ProtectionDomain, QpInitAttr};

// Re-exports
pub use interface::addrinfo::{AddrFamily, AddrInfo, AddrInfoFlags, AddrInfoHints, PortSpace};

#[derive(Clone)]
pub(crate) struct CmIdBuilder<'pd, 'ctx, 'scq, 'rcq, 'srq> {
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
    pub(crate) fn new() -> Self {
        CmIdBuilder {
            handle: interface::CmId(interface::Handle::INVALID),
            pd: None,
            qp_init_attr: Default::default(),
        }
    }

    pub(crate) fn set_pd(&mut self, pd: &'pd ProtectionDomain) -> &mut Self {
        self.pd = Some(pd);
        self
    }

    pub(crate) fn set_qp_init_attr<'a>(
        &mut self,
        qp_init_attr: &'a QpInitAttr<'ctx, 'scq, 'rcq, 'srq>,
    ) -> &mut Self {
        self.qp_init_attr = qp_init_attr.clone();
        self
    }

    pub(crate) fn set_qp_context(&mut self, qp_context: &'ctx (dyn Any + Send + Sync)) -> &mut Self {
        self.qp_init_attr.qp_context = Some(qp_context);
        self
    }

    pub(crate) fn set_send_cq(&mut self, send_cq: &'scq uverbs::CompletionQueue) -> &mut Self {
        self.qp_init_attr.send_cq = Some(send_cq);
        self
    }

    pub(crate) fn set_recv_cq(&mut self, recv_cq: &'rcq uverbs::CompletionQueue) -> &mut Self {
        self.qp_init_attr.recv_cq = Some(recv_cq);
        self
    }

    pub(crate) fn set_cap(&mut self, cap: uverbs::QpCapability) -> &mut Self {
        self.qp_init_attr.cap = cap;
        self
    }

    pub(crate) fn set_max_send_wr(&mut self, max_send_wr: u32) -> &mut Self {
        self.qp_init_attr.cap.max_send_wr = max_send_wr;
        self
    }

    pub(crate) fn set_max_recv_wr(&mut self, max_recv_wr: u32) -> &mut Self {
        self.qp_init_attr.cap.max_recv_wr = max_recv_wr;
        self
    }

    pub(crate) fn set_max_send_sge(&mut self, max_send_sge: u32) -> &mut Self {
        self.qp_init_attr.cap.max_send_sge = max_send_sge;
        self
    }

    pub(crate) fn set_max_recv_sge(&mut self, max_recv_sge: u32) -> &mut Self {
        self.qp_init_attr.cap.max_recv_sge = max_recv_sge;
        self
    }

    pub(crate) fn set_max_inline_data(&mut self, max_inline_data: u32) -> &mut Self {
        self.qp_init_attr.cap.max_inline_data = max_inline_data;
        self
    }

    pub(crate) fn signal_all(&mut self) -> &mut Self {
        self.qp_init_attr.sq_sig_all = true;
        self
    }

    pub(crate) async fn bind<A: ToSocketAddrs>(&self, addr: A) -> Result<CmIdListener, Error> {
        let listen_addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or(Error::NoAddrResolved)?;
        let ops = get_ops();
        // create_id
        let cmid = ops.create_id(PortSpace::TCP).await?;
        assert!(cmid.qp.is_none());
        // drop guard, automatically drop if any of the following step fails
        let drop_cmid = DropCmId(cmid.handle);
        // bind_addr
        ops.bind_addr(cmid.handle.0, &listen_addr)?;
        // listen
        ops.listen(cmid.handle.0, 512)?;
        mem::forget(drop_cmid);
        Ok(CmIdListener { handle: cmid.handle })
    }

    pub(crate) async fn resolve_route<A: ToSocketAddrs>(&self, addr: A) -> Result<CmIdBuilder<'pd, 'ctx, 'scq, 'rcq, 'srq>, Error> {
        let connect_addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or(Error::NoAddrResolved)?;
        let ops = get_ops();
        // create_id
        let cmid = ops.create_id(PortSpace::TCP).await?;
        assert!(cmid.qp.is_none());
        // drop guard, automatically drop if any of the following step fails
        let drop_cmid = DropCmId(cmid.handle);
        // resolve_addr
        ops.resolve_addr(cmid.handle.0, &connect_addr).await?;
        // resolve_route
        ops.resolve_route(cmid.handle.0, 2000).await?;
        assert!(cmid.qp.is_none());
        let mut builder = self.clone();
        builder.handle = cmid.handle;
        mem::forget(drop_cmid);
        Ok(builder)
    }

    pub(crate) fn build(&self) -> Result<PreparedCmId, Error> {
        // create_qp
        let pd = self.pd.map(|pd| pd.inner);
        let qp = get_ops().cm_create_qp(self.handle.0, pd.as_ref(), &interface::QpInitAttr::from_borrow(&self.qp_init_attr))?;
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
        get_ops()
            .destroy_id(&self.0)
            .unwrap_or_else(|e| eprintln!("Destroying CmId: {}", e));
    }
}

// COMMENT(cjr): here we remove some functionalities of the CmIdListener from the libkoala so that
// the 5 lifetime generices does not need to propagate.
pub(crate) struct CmIdListener {
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
    pub(crate) async fn bind<A: ToSocketAddrs>(addr: A) -> Result<Self, Error> {
        CmIdBuilder::new().bind(addr).await
    }

    pub(crate) async fn get_request<'pd, 'ctx, 'scq, 'rcq, 'srq>(
        &self,
    ) -> Result<CmIdBuilder<'pd, 'ctx, 'scq, 'rcq, 'srq>, Error> {
        let cmid = get_ops().get_request(self.handle.0).await?;
        assert!(cmid.qp.is_none());
        let mut builder = CmIdBuilder::new();
        builder.handle = cmid.handle;
        Ok(builder)
    }

    pub(crate) fn try_get_request<'pd, 'ctx, 'scq, 'rcq, 'srq>(
        &self,
    ) -> Result<Option<CmIdBuilder<'pd, 'ctx, 'scq, 'rcq, 'srq>>, Error> {
        let maybe_cmid = get_ops().try_get_request(self.handle.0)?;
        if let Some(cmid) = maybe_cmid.as_ref() {
            assert!(cmid.qp.is_none());
            let mut builder = CmIdBuilder::new();
            builder.handle = cmid.handle;
            Ok(Some(builder))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug)]
pub(crate) struct PreparedCmId {
    pub(crate) inner: Inner,
}

impl AsHandle for PreparedCmId {
    #[inline]
    fn as_handle(&self) -> Handle {
        self.inner.handle.0
    }
}

impl PreparedCmId {
    pub(crate) async fn accept<'a>(self, conn_param: Option<&'a ConnParam<'a>>) -> Result<CmId, Error> {
        let conn_param = conn_param.map(|param| interface::ConnParam::from_borrow(&param));
        get_ops().accept(self.inner.handle.0, conn_param.as_ref()).await?;
        Ok(CmId { inner: self.inner })
    }

    pub(crate) async fn connect<'a>(self, conn_param: Option<&'a ConnParam<'a>>) -> Result<CmId, Error> {
        let conn_param = conn_param.map(|param| interface::ConnParam::from_borrow(&param));
        get_ops().connect(self.inner.handle.0, conn_param.as_ref()).await.map_err(Error::Connect)?;
        Ok(CmId { inner: self.inner })
    }
}

#[derive(Debug)]
pub(crate) struct CmId {
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
        futures::executor::block_on(async {
            get_ops().disconnect(&self.inner.handle).await.unwrap_or_else(|e| eprintln!("Disconnecting CmId: {}", e));
        });
    }
}

impl CmId {
    pub(crate) async fn resolve_route<'pd, 'ctx, 'scq, 'rcq, 'srq, A: ToSocketAddrs>(
        addr: A,
    ) -> Result<CmIdBuilder<'pd, 'ctx, 'scq, 'rcq, 'srq>, Error> {
        CmIdBuilder::new().resolve_route(addr).await
    }
}

macro_rules! impl_for_cmid {
    ($type:ty, $member:ident) => {
        impl $type {
            pub(crate) fn qp(&self) -> &uverbs::QueuePair {
                &self.$member.qp
            }

            pub(crate) fn alloc_msgs<T: Sized + Copy>(
                &self,
                len: usize,
            ) -> Result<uverbs::MemoryRegion<T>, Error> {
                self.$member.alloc_msgs(len)
            }

            pub(crate) fn alloc_write<T: Sized + Copy>(
                &self,
                len: usize,
            ) -> Result<uverbs::MemoryRegion<T>, Error> {
                self.$member.alloc_write(len)
            }

            pub(crate) fn alloc_read<T: Sized + Copy>(
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
    pub(crate) fn alloc_msgs<T: Sized + Copy>(
        &self,
        len: usize,
    ) -> Result<uverbs::MemoryRegion<T>, Error> {
        self.qp.pd.allocate(len, AccessFlags::LOCAL_WRITE)
    }

    pub(crate) fn alloc_write<T: Sized + Copy>(
        &self,
        len: usize,
    ) -> Result<uverbs::MemoryRegion<T>, Error> {
        self.qp
            .pd
            .allocate(len, AccessFlags::LOCAL_WRITE | AccessFlags::REMOTE_WRITE)
    }

    pub(crate) fn alloc_read<T: Sized + Copy>(
        &self,
        len: usize,
    ) -> Result<uverbs::MemoryRegion<T>, Error> {
        self.qp
            .pd
            .allocate(len, AccessFlags::LOCAL_WRITE | AccessFlags::REMOTE_READ)
    }
}
