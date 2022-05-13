//! Providing the API implemention for both TransportEngine and RpcAdapter.
//! The API design requires a bit finesse.
//!
//! TODO(cjr): remove the following docs.
//! Option 1: Using the native data types in rdma. This gives the best performance,
//! but less portability and development velocity
//!
//! Option 2: Using the general API in interface. This scarifices the performance a bit,
//! but prioritizes portability and agility. We will go this way, and see if the performance
//! is good or not.
use std::io;
use std::net::SocketAddr;
use std::slice;
use std::sync::Arc;

use log::trace;

use interface::{returned, AsHandle, Handle};
use rdma::ibv;
use rdma::mr::MemoryRegion;
use rdma::rdmacm;
use rdma::rdmacm::CmId;

use super::state::{Resource, State};
use super::{ApiError, DatapathError};
use crate::engine::future;

pub(crate) type Result<T> = std::result::Result<T, ApiError>;

// TODO(cjr): API tracing. Do it later.
// struct ApiGuard;
//
// impl ApiGuard {
//     #[inline]
//     fn on_enter() { }
//
//     #[inline]
//     fn on_exit(&self) { }
// }
//
// impl Drop for ApiGuard {
//     fn drop(&mut self) {
//         self.on_exit();
//     }
// }

pub(crate) struct Ops {
    pub(crate) state: State,
}

impl Ops {
    pub(crate) fn new(state: State) -> Self {
        Self { state }
    }

    #[inline]
    pub(crate) fn resource(&self) -> &Resource {
        &self.state.shared.resource
    }
}

// Datapath APIs
impl Ops {
    #[inline]
    pub(crate) unsafe fn post_recv(
        &self,
        cmid_handle: Handle,
        mr: &rdmacm::MemoryRegion,
        range: ipc::buf::Range,
        wr_id: u64,
    ) -> std::result::Result<(), DatapathError> {
        // trace!(
        //     "cmid_handle: {:?}, wr_id: {:?}, range: {:x?}, mr_handle: {:?}",
        //     cmid_handle,
        //     wr_id,
        //     user_buf,
        //     mr_handle
        // );
        let cmid = self.resource().cmid_table.get_dp(&cmid_handle)?;

        // since post_recv itself is already unsafe, it is the user's responsibility to
        // make sure the received data is valid. The user must avoid post_recv a same
        // buffer multiple times (e.g. from a single thread or from multiple threads)
        // without any synchronization.
        // let rdma_mr = rdmacm::MemoryRegion::from(mr);
        let buf = &mr[range.offset as usize..(range.offset + range.len) as usize];
        let buf_mut = slice::from_raw_parts_mut(buf.as_ptr() as _, buf.len());
        cmid.post_recv(wr_id, buf_mut, &mr)
            .map_err(DatapathError::RdmaCm)?;
        Ok(())
    }

    #[inline]
    pub(crate) unsafe fn post_send(
        &self,
        cmid_handle: Handle,
        mr: &rdmacm::MemoryRegion,
        range: ipc::buf::Range,
        wr_id: u64,
        send_flags: interface::SendFlags,
    ) -> std::result::Result<(), DatapathError> {
        // trace!(
        //     "cmid_handle: {:?}, wr_id: {:?}, range: {:x?}, mr_handle: {:?}, send_flags: {:?}",
        //     cmid_handle,
        //     wr_id,
        //     user_buf,
        //     mr_handle,
        //     send_flags,
        // );
        let cmid = self.resource().cmid_table.get_dp(&cmid_handle)?;

        // let rdma_mr = rdmacm::MemoryRegion::from(mr);
        let buf = &mr[range.offset as usize..(range.offset + range.len) as usize];

        let flags: ibv::SendFlags = send_flags.into();
        cmid.post_send(wr_id, buf, &mr, flags.0)
            .map_err(DatapathError::RdmaCm)?;
        Ok(())
    }

    #[inline]
    pub(crate) unsafe fn post_send_with_imm(
        &self,
        cmid_handle: Handle,
        mr: &rdmacm::MemoryRegion,
        range: ipc::buf::Range,
        wr_id: u64,
        send_flags: interface::SendFlags,
        imm: u32,
    ) -> std::result::Result<(), DatapathError> {
        let cmid = self.resource().cmid_table.get_dp(&cmid_handle)?;

        // let rdma_mr = rdmacm::MemoryRegion::from(&mr);
        let buf = &mr[range.offset as usize..(range.offset + range.len) as usize];

        let flags: ibv::SendFlags = send_flags.into();
        cmid.post_send_with_imm(wr_id, buf, &mr, flags.0, imm)
            .map_err(DatapathError::RdmaCm)?;
        Ok(())
    }

    #[inline]
    pub(crate) unsafe fn post_write(
        &self,
        cmid_handle: Handle,
        mr: &rdmacm::MemoryRegion,
        range: ipc::buf::Range,
        wr_id: u64,
        rkey: interface::RemoteKey,
        remote_offset: u64,
        send_flags: interface::SendFlags,
    ) -> std::result::Result<(), DatapathError> {
        let cmid = self.resource().cmid_table.get_dp(&cmid_handle)?;

        // let rdma_mr = rdmacm::MemoryRegion::from(mr);
        let buf = &mr[range.offset as usize..(range.offset + range.len) as usize];
        let remote_addr = rkey.addr + remote_offset;

        let flags: ibv::SendFlags = send_flags.into();
        cmid.post_write(wr_id, buf, &mr, flags.0, remote_addr, rkey.rkey)
            .map_err(DatapathError::RdmaCm)?;

        Ok(())
    }

    #[inline]
    pub(crate) unsafe fn post_read(
        &self,
        cmid_handle: Handle,
        mr: &rdmacm::MemoryRegion,
        range: ipc::buf::Range,
        wr_id: u64,
        rkey: interface::RemoteKey,
        remote_offset: u64,
        send_flags: interface::SendFlags,
    ) -> std::result::Result<(), DatapathError> {
        let cmid = self.resource().cmid_table.get_dp(&cmid_handle)?;

        let remote_addr = rkey.addr + remote_offset;
        let flags: ibv::SendFlags = send_flags.into();

        // let rdma_mr = rdmacm::MemoryRegion::from(mr);
        let buf = &mr[range.offset as usize..(range.offset + range.len) as usize];
        let buf_mut = slice::from_raw_parts_mut(buf.as_ptr() as _, buf.len());
        cmid.post_read(wr_id, buf_mut, &mr, flags.0, remote_addr, rkey.rkey)
            .map_err(DatapathError::RdmaCm)?;
        Ok(())
    }

    #[inline]
    pub(crate) fn poll_cq(
        &self,
        cq_handle: &interface::CompletionQueue,
        wc: &mut Vec<interface::WorkCompletion>,
    ) -> std::result::Result<(), DatapathError> {
        let cq = self.resource().cq_table.get_dp(&cq_handle.0)?;
        if wc.capacity() == 0 {
            log::warn!("wc capacity is zero");
            return Ok(());
        }
        // Safety: this is fine here because we will resize the wc to the number of elements it really gets
        let mut wc_slice =
            unsafe { slice::from_raw_parts_mut(wc.as_mut_ptr().cast(), wc.capacity()) };
        match cq.poll(&mut wc_slice) {
            Ok(completions) => {
                unsafe { wc.set_len(completions.len()) };
                Ok(())
            }
            Err(rdma::ibv::PollCqError) => {
                unsafe { wc.set_len(0) };
                Err(DatapathError::Ibv(io::Error::last_os_error()))
            }
        }
    }
}

// Control path APIs
impl Ops {
    pub(crate) fn get_addr_info(
        &mut self,
        node: Option<&str>,
        service: Option<&str>,
        hints: Option<&interface::addrinfo::AddrInfoHints>,
    ) -> Result<interface::addrinfo::AddrInfo> {
        trace!(
            "GetAddrInfo, node: {:?}, service: {:?}, hints: {:?}",
            node,
            service,
            hints,
        );

        let hints = hints.map(|h| rdmacm::AddrInfoHints::from(*h));
        let ret = rdmacm::AddrInfo::getaddrinfo(node, service, hints.as_ref());
        match ret {
            Ok(ai) => Ok(ai.into()),
            Err(e) => Err(ApiError::GetAddrInfo(e)),
        }
    }

    pub(crate) fn create_ep(
        &self,
        ai: &interface::addrinfo::AddrInfo,
        pd: Option<&interface::ProtectionDomain>,
        qp_init_attr: Option<&interface::QpInitAttr>,
    ) -> Result<returned::CmId> {
        trace!(
            "CreateEp, ai: {:?}, pd: {:?}, qp_init_attr: {:?}",
            ai,
            pd,
            qp_init_attr
        );

        let (pd, qp_init_attr) = self.get_qp_params(pd, qp_init_attr)?;
        match CmId::create_ep(&ai.clone().into(), pd.as_deref(), qp_init_attr.as_ref()) {
            Ok((cmid, qp)) => {
                let cmid_handle = self.resource().insert_cmid(cmid)?;
                let ret_qp = if let Some(qp) = qp {
                    let handles = self.resource().insert_qp(qp)?;
                    Some(prepare_returned_qp(handles))
                } else {
                    None
                };
                Ok(returned::CmId {
                    handle: interface::CmId(cmid_handle),
                    qp: ret_qp,
                })
            }
            Err(e) => Err(ApiError::RdmaCm(e)),
        }
    }

    pub(crate) async fn create_id(
        &self,
        port_space: interface::addrinfo::PortSpace,
    ) -> Result<returned::CmId> {
        trace!("CreateId, port_space: {:?}", port_space);

        // create a new event channel for each cmid
        let channel = rdmacm::EventChannel::create_event_channel().map_err(ApiError::RdmaCm)?;

        // set nonblocking and register channel to the IO reactor
        channel.set_nonblocking(true).map_err(ApiError::RdmaCm)?;
        let channel_handle = channel.as_handle();

        self.register_event_channel(channel_handle, &channel)
            .await?;

        // TODO(cjr): this is safe because event_channel will be stored in the
        // ResourceTable
        let ps: rdmacm::PortSpace = port_space.into();
        let cmid = unsafe { CmId::create_id(Some(&channel), 0, ps.0) }.map_err(ApiError::RdmaCm)?;

        // insert event_channel
        // TODO(cjr): think over it. What if any exception happen in between any of these
        // operations? How to safely/correctly rollback?
        self.resource()
            .event_channel_table
            .insert(channel_handle, channel)?;

        // insert cmid after event_channel is inserted
        let new_cmid_handle = self.resource().insert_cmid(cmid)?;
        Ok(returned::CmId {
            handle: interface::CmId(new_cmid_handle),
            qp: None,
        })
    }

    pub(crate) fn listen(&self, cmid_handle: Handle, backlog: i32) -> Result<()> {
        trace!(
            "Listen, cmid_handle: {:?}, backlog: {}",
            cmid_handle,
            backlog
        );

        let listener = self.resource().cmid_table.get(&cmid_handle)?;
        listener.listen(backlog).map_err(ApiError::RdmaCm)?;
        Ok(())
    }

    fn handle_connect_request(&self, event: rdmacm::CmEvent) -> Result<returned::CmId> {
        let (new_cmid, new_qp) = event.get_request();

        let ret_qp = if let Some(qp) = new_qp {
            let handles = self.resource().insert_qp(qp)?;
            Some(prepare_returned_qp(handles))
        } else {
            None
        };
        let new_cmid_handle = self.resource().insert_cmid(new_cmid)?;
        Ok(returned::CmId {
            handle: interface::CmId(new_cmid_handle),
            qp: ret_qp,
        })
    }

    pub(crate) async fn get_request(&self, listener_handle: Handle) -> Result<returned::CmId> {
        trace!("GetRequest, listener_handle: {:?}", listener_handle);

        let event_type = rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_CONNECT_REQUEST;
        let listener_cmid = self.resource().cmid_table.get(&listener_handle)?;
        let ec_handle = listener_cmid.event_channel().as_handle();
        let event = self.wait_cm_event(&ec_handle, event_type).await?;

        // The following part executes when an cm_event occurs
        self.handle_connect_request(event)
    }

    pub(crate) fn try_get_request(
        &self,
        listener_handle: Handle,
    ) -> Result<Option<returned::CmId>> {
        trace!("TryGetRequest, listener_handle: {:?}", listener_handle);

        let event_type = rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_CONNECT_REQUEST;
        let listener_cmid = self.resource().cmid_table.get(&listener_handle)?;
        let ec_handle = listener_cmid.event_channel().as_handle();
        let res = self.try_get_cm_event(&ec_handle, event_type);
        if res.is_none() {
            return Ok(None);
        }

        let event = res.unwrap()?;

        // The following part executes when an cm_event occurs
        Some(self.handle_connect_request(event)).transpose()
    }

    pub(crate) async fn accept(
        &self,
        cmid_handle: Handle,
        conn_param: Option<&interface::ConnParam>,
    ) -> Result<()> {
        trace!(
            "Accept, cmid_handle: {:?}, conn_param: {:?}",
            cmid_handle,
            conn_param
        );

        let cmid = self.resource().cmid_table.get(&cmid_handle)?;
        cmid.accept(self.get_conn_param(conn_param).as_ref())
            .map_err(ApiError::RdmaCm)?;

        // wait until the accept is done
        let event_type = rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_ESTABLISHED;
        let ec_handle = cmid.event_channel().as_handle();
        let _event = self.wait_cm_event(&ec_handle, event_type).await?;

        Ok(())
    }

    pub(crate) async fn connect(
        &self,
        cmid_handle: Handle,
        conn_param: Option<&interface::ConnParam>,
    ) -> Result<()> {
        trace!(
            "Connect, cmid_handle: {:?}, conn_param: {:?}",
            cmid_handle,
            conn_param
        );

        let cmid = self.resource().cmid_table.get(&cmid_handle)?;
        cmid.connect(self.get_conn_param(conn_param).as_ref())
            .map_err(ApiError::RdmaCm)?;

        // wait until the accept is done
        let event_type = rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_ESTABLISHED;
        let ec_handle = cmid.event_channel().as_handle();
        let _event = self.wait_cm_event(&ec_handle, event_type).await?;

        Ok(())
    }

    pub(crate) fn bind_addr(&self, cmid_handle: Handle, sockaddr: &SocketAddr) -> Result<()> {
        trace!(
            "BindAddr, cmid_handle: {:?}, sockaddr: {:?}",
            cmid_handle,
            sockaddr
        );

        let cmid = self.resource().cmid_table.get(&cmid_handle)?;
        cmid.bind_addr(sockaddr).map_err(ApiError::RdmaCm)?;
        Ok(())
    }

    pub(crate) async fn resolve_addr(
        &self,
        cmid_handle: Handle,
        sockaddr: &SocketAddr,
    ) -> Result<()> {
        trace!(
            "ResolveAddr: cmid_handle: {:?}, sockaddr: {:?}",
            cmid_handle,
            sockaddr
        );

        let cmid = self.resource().cmid_table.get(&cmid_handle)?;
        cmid.resolve_addr(sockaddr).map_err(ApiError::RdmaCm)?;

        let event_type = rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_ADDR_RESOLVED;
        let ec_handle = cmid.event_channel().as_handle();
        log::debug!("before wait_cm_event");
        let _event = self.wait_cm_event(&ec_handle, event_type).await?;
        log::debug!("after wait_cm_event");

        Ok(())
    }

    pub(crate) async fn resolve_route(&self, cmid_handle: Handle, timeout_ms: i32) -> Result<()> {
        trace!(
            "ResolveRoute: cmid_handle: {:?}, timeout_ms: {:?}",
            cmid_handle,
            timeout_ms
        );

        let cmid = self.resource().cmid_table.get(&cmid_handle)?;
        cmid.resolve_route(timeout_ms).map_err(ApiError::RdmaCm)?;

        let event_type = rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_ROUTE_RESOLVED;
        let ec_handle = cmid.event_channel().as_handle();
        let _event = self.wait_cm_event(&ec_handle, event_type).await?;

        Ok(())
    }

    pub(crate) fn cm_create_qp(
        &self,
        cmid_handle: Handle,
        pd: Option<&interface::ProtectionDomain>,
        qp_init_attr: &interface::QpInitAttr,
    ) -> Result<returned::QueuePair> {
        trace!(
            "CmCreateQp, cmid_handle: {:?}, pd: {:?}, qp_init_attr: {:?}",
            cmid_handle,
            pd,
            qp_init_attr
        );

        let cmid = self.resource().cmid_table.get(&cmid_handle)?;

        let pd = pd.cloned().or_else(|| {
            // use the default pd of the corresponding device
            let sgid = cmid.sgid();
            Some(
                self.resource()
                    .default_pd(&sgid)
                    .expect("Something is wrong"),
            )
        });

        let (pd, qp_init_attr) = self.get_qp_params(pd.as_ref(), Some(qp_init_attr))?;
        let qp = cmid
            .create_qp(pd.as_deref(), qp_init_attr.as_ref())
            .map_err(ApiError::RdmaCm)?;
        let handles = self.resource().insert_qp(qp)?;
        Ok(prepare_returned_qp(handles))
    }

    // NOTE(cjr): reg_mr does not insert the MR to its table.
    pub(crate) fn reg_mr(
        &self,
        pd: &interface::ProtectionDomain,
        nbytes: usize,
        access: interface::AccessFlags,
    ) -> Result<MemoryRegion> {
        trace!(
            "RegMr, pd: {:?}, nbytes: {}, access: {:?}",
            pd,
            nbytes,
            access
        );

        let pd = self.resource().pd_table.get(&pd.0)?;
        let mr = MemoryRegion::new(&pd, nbytes, access)
            .map_err(ApiError::MemoryRegion)
            .expect("something is wrong; remove this expect() later");
        Ok(mr)
    }

    // NOTE(cjr): There is no API for dereg_mr. All the user needs to do is to drop it.

    pub(crate) fn create_cq(
        &self,
        ctx: &interface::VerbsContext,
        min_cq_entries: i32,
        cq_context: u64,
    ) -> Result<returned::CompletionQueue> {
        trace!(
            "CreateCq, ctx: {:?}, min_cq_entries: {:?}, cq_context: {:?}",
            ctx,
            min_cq_entries,
            cq_context
        );

        use super::state::DEFAULT_CTXS;
        let index = ctx.0 .0 as usize;
        if index >= DEFAULT_CTXS.len() {
            return Err(ApiError::NotFound);
        }

        let verbs = &DEFAULT_CTXS[index].pinned_ctx.verbs;
        let cq = verbs
            .create_cq(min_cq_entries, cq_context as _)
            .map_err(ApiError::Ibv)?;
        let handle = cq.as_handle();
        self.resource()
            .cq_table
            .occupy_or_create_resource(handle, cq);

        Ok(returned::CompletionQueue {
            handle: interface::CompletionQueue(handle),
        })
    }

    pub(crate) fn dealloc_pd(&self, pd: &interface::ProtectionDomain) -> Result<()> {
        trace!("DeallocPd, pd: {:?}", pd);
        self.resource().pd_table.close_resource(&pd.0)?;
        Ok(())
    }

    pub(crate) fn destroy_cq(&self, cq: &interface::CompletionQueue) -> Result<()> {
        trace!("DestroyCq, cq: {:?}", cq);
        self.resource().cq_table.close_resource(&cq.0)?;
        Ok(())
    }

    pub(crate) fn destroy_qp(&self, qp: &interface::QueuePair) -> Result<()> {
        trace!("DestroyQp, qp: {:?}", qp);
        self.resource().qp_table.close_resource(&qp.0)?;
        Ok(())
    }

    pub(crate) async fn disconnect(&self, cmid: &interface::CmId) -> Result<()> {
        trace!("Disconnect, cmid: {:?}", cmid);

        let cmid_handle = cmid.0;
        let cmid = self.resource().cmid_table.get(&cmid_handle)?;
        cmid.disconnect().map_err(ApiError::RdmaCm)?;

        let event_type = rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_DISCONNECTED;
        let ec_handle = cmid.event_channel().as_handle();
        let _event = self.wait_cm_event(&ec_handle, event_type).await?;

        Ok(())
    }

    pub(crate) fn destroy_id(&self, cmid: &interface::CmId) -> Result<()> {
        trace!("DestroyId, cmid: {:?}", cmid);
        self.resource().cmid_table.close_resource(&cmid.0)?;
        Ok(())
    }

    pub(crate) fn open_pd(&self, pd: &interface::ProtectionDomain) -> Result<()> {
        trace!("OpenPd, pd: {:?}", pd);
        self.resource().pd_table.open_resource(&pd.0)?;
        Ok(())
    }

    pub(crate) fn open_cq(&self, cq: &interface::CompletionQueue) -> Result<u32> {
        trace!("OpenCq, cq: {:?}", cq);
        self.resource().cq_table.open_resource(&cq.0)?;
        let cq = self.resource().cq_table.get(&cq.0)?;
        Ok(cq.capacity())
    }

    pub(crate) fn open_qp(&self, qp: &interface::QueuePair) -> Result<()> {
        trace!("OpenQp, qp: {:?}", qp);
        self.resource().qp_table.open_resource(&qp.0)?;
        Ok(())
    }

    pub(crate) fn get_default_pds(&self) -> Result<Vec<returned::ProtectionDomain>> {
        trace!("GetDefaultPds");
        let pds = self
            .resource()
            .default_pds
            .lock()
            .iter()
            .map(|(pd, _gids)| returned::ProtectionDomain { handle: *pd })
            .collect();
        Ok(pds)
    }

    pub(crate) fn get_default_contexts(&self) -> Result<Vec<returned::VerbsContext>> {
        trace!("GetDefaultContexts");
        use super::state::DEFAULT_CTXS;
        let ctx_list = DEFAULT_CTXS
            .iter()
            .enumerate()
            .map(|(i, _)| returned::VerbsContext {
                handle: interface::VerbsContext(Handle(i as _)),
            })
            .collect();
        Ok(ctx_list)
    }

    pub(crate) fn create_mr_on_demand_paging(
        &self,
        pd_handle: &interface::ProtectionDomain,
    ) -> Result<rdmacm::MemoryRegion<'static>> {
        let pd = self.resource().pd_table.get(&pd_handle.0)?;
        Ok(rdmacm::MemoryRegion::new_on_demand_paging(pd.pd()).map_err(ApiError::Ibv)?)
    }

    fn get_qp_params(
        &self,
        pd_handle: Option<&interface::ProtectionDomain>,
        qp_init_attr: Option<&interface::QpInitAttr>,
    ) -> Result<(
        Option<Arc<ibv::ProtectionDomain<'static>>>,
        Option<rdma::ffi::ibv_qp_init_attr>,
    )> {
        let pd = if let Some(h) = pd_handle {
            Some(self.resource().pd_table.get(&h.0)?)
        } else {
            None
        };
        let qp_init_attr = if let Some(a) = qp_init_attr {
            let send_cq = if let Some(ref h) = a.send_cq {
                Some(self.resource().cq_table.get(&h.0)?)
            } else {
                None
            };
            let recv_cq = if let Some(ref h) = a.recv_cq {
                Some(self.resource().cq_table.get(&h.0)?)
            } else {
                None
            };
            let attr = ibv::QpInitAttr {
                qp_context: 0,
                send_cq: send_cq.as_deref(),
                recv_cq: recv_cq.as_deref(),
                cap: a.cap.into(),
                qp_type: a.qp_type.into(),
                sq_sig_all: a.sq_sig_all,
            };
            Some(attr.to_ibv_qp_init_attr())
        } else {
            None
        };
        Ok((pd, qp_init_attr))
    }

    fn get_conn_param(
        &self,
        conn_param: Option<&interface::ConnParam>,
    ) -> Option<rdma::ffi::rdma_conn_param> {
        conn_param.map(|param| rdma::ffi::rdma_conn_param {
            private_data: param
                .private_data
                .as_ref()
                .map_or(std::ptr::null(), |data| data.as_ptr())
                as *const _,
            private_data_len: param.private_data.as_ref().map_or(0, |data| data.len()) as u8,
            responder_resources: param.responder_resources,
            initiator_depth: param.initiator_depth,
            flow_control: param.flow_control,
            retry_count: param.retry_count,
            rnr_retry_count: param.rnr_retry_count,
            srq: param.srq,
            qp_num: param.qp_num,
        })
    }

    async fn register_event_channel(
        &self,
        channel_handle: Handle,
        channel: &rdmacm::EventChannel,
    ) -> Result<()> {
        self.state
            .shared
            .cm_manager
            .lock()
            .await
            .register_event_channel(channel_handle, channel)?;
        Ok(())
    }

    fn get_one_cm_event(
        &self,
        event_channel_handle: &Handle,
        event_type: rdma::ffi::rdma_cm_event_type::Type,
    ) -> Option<rdmacm::CmEvent> {
        self.state
            .shared
            .cm_manager
            .try_lock()
            .ok()
            .and_then(|mut manager| manager.get_one_cm_event(event_channel_handle, event_type))
    }

    fn pop_first_cm_error(&self) -> Option<ApiError> {
        self.state
            .shared
            .cm_manager
            .try_lock()
            .ok()
            .and_then(|mut manager| manager.first_error())
    }

    fn try_get_cm_event(
        &self,
        event_channel_handle: &Handle,
        event_type: rdma::ffi::rdma_cm_event_type::Type,
    ) -> Option<Result<rdmacm::CmEvent>> {
        log::debug!(
            "try_get_cm_event, ec_handle: {:?}, event_type: {:?}",
            event_channel_handle,
            event_type
        );
        if let Some(cm_event) = self.get_one_cm_event(event_channel_handle, event_type) {
            log::trace!(
                "try_get_cm_event got, ec_handle: {:?}, cm_event: {:?}",
                event_channel_handle,
                cm_event
            );
            use std::cmp;
            match cm_event.status().cmp(&0) {
                cmp::Ordering::Equal => {}
                cmp::Ordering::Less => {
                    return Some(Err(ApiError::RdmaCm(io::Error::from_raw_os_error(
                        -cm_event.status(),
                    ))));
                }
                cmp::Ordering::Greater => return Some(Err(ApiError::Transport(cm_event.status()))),
            }
            return Some(Ok(cm_event));
        }
        if let Some(err) = self.pop_first_cm_error() {
            log::warn!(
                "try_get_cm_event, got error: ec_handle: {:?}, err: {:?}",
                event_channel_handle,
                err
            );
            return Some(Err(err));
        }
        None
    }

    async fn wait_cm_event(
        &self,
        event_channel_handle: &Handle,
        event_type: rdma::ffi::rdma_cm_event_type::Type,
    ) -> Result<rdmacm::CmEvent> {
        loop {
            if let Some(res) = self.try_get_cm_event(event_channel_handle, event_type) {
                return res;
            }
            future::yield_now().await;
        }
    }
}

fn prepare_returned_qp(handles: (Handle, Handle, Handle, Handle)) -> returned::QueuePair {
    let (qp_handle, pd_handle, scq_handle, rcq_handle) = handles;
    returned::QueuePair {
        handle: interface::QueuePair(qp_handle),
        pd: returned::ProtectionDomain {
            handle: interface::ProtectionDomain(pd_handle),
        },
        send_cq: returned::CompletionQueue {
            handle: interface::CompletionQueue(scq_handle),
        },
        recv_cq: returned::CompletionQueue {
            handle: interface::CompletionQueue(rcq_handle),
        },
    }
}
