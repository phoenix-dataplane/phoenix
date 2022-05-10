//! Providing the API implemention for both TransportEngine and RpcAdapter.
//! The API design requires a bit finesse.
//!
//! Option 1: Using the native data types in rdma. This gives the best performance,
//! but less portability and development velocity
//!
//! Option 2: Using the general API in interface. This scarifices the performance a bit,
//! but prioritizes portability and agility. We will go this way, and see if the performance
//! is good or not.
use std::net::SocketAddr;
use std::sync::Arc;

use log::trace;

use interface::{returned, AsHandle, Handle};
use rdma::ibv;
use rdma::rdmacm;
use rdma::rdmacm::CmId;

use super::engine::TransportEngine;
use super::ApiError;
use crate::engine::future;

pub(crate) type Result<T> = std::result::Result<T, ApiError>;

// TODO(cjr): API tracing. Finish it later.
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

// Datapath APIs
impl TransportEngine {
    // #[inline]
    // pub(crate) fn post_recv()
}

// Control path APIs
impl TransportEngine {
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
        &mut self,
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
                let cmid_handle = self.state.resource().insert_cmid(cmid)?;
                let ret_qp = if let Some(qp) = qp {
                    let handles = self.state.resource().insert_qp(qp)?;
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
        &mut self,
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
        self.state
            .resource()
            .event_channel_table
            .insert(channel_handle, channel)?;

        // insert cmid after event_channel is inserted
        let new_cmid_handle = self.state.resource().insert_cmid(cmid)?;
        Ok(returned::CmId {
            handle: interface::CmId(new_cmid_handle),
            qp: None,
        })
    }

    pub(crate) fn listen(&mut self, cmid_handle: Handle, backlog: i32) -> Result<()> {
        trace!(
            "Listen, cmid_handle: {:?}, backlog: {}",
            cmid_handle,
            backlog
        );

        let listener = self.state.resource().cmid_table.get(&cmid_handle)?;
        listener.listen(backlog).map_err(ApiError::RdmaCm)?;
        Ok(())
    }

    fn handle_connect_request(&mut self, event: rdmacm::CmEvent) -> Result<returned::CmId> {
        let (new_cmid, new_qp) = event.get_request();

        let ret_qp = if let Some(qp) = new_qp {
            let handles = self.state.resource().insert_qp(qp)?;
            Some(prepare_returned_qp(handles))
        } else {
            None
        };
        let new_cmid_handle = self.state.resource().insert_cmid(new_cmid)?;
        Ok(returned::CmId {
            handle: interface::CmId(new_cmid_handle),
            qp: ret_qp,
        })
    }

    pub(crate) async fn get_request(&mut self, listener_handle: Handle) -> Result<returned::CmId> {
        trace!("GetRequest, listener_handle: {:?}", listener_handle);

        let event_type = rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_CONNECT_REQUEST;
        let event = self.wait_cm_event(&listener_handle, event_type).await?;

        // The following part executes when an cm_event occurs
        self.handle_connect_request(event)
    }

    pub(crate) fn try_get_request(
        &mut self,
        listener_handle: Handle,
    ) -> Result<Option<returned::CmId>> {
        trace!("TryGetRequest, listener_handle: {:?}", listener_handle);

        let event_type = rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_CONNECT_REQUEST;
        let res = self.try_get_cm_event(&listener_handle, event_type);
        if res.is_none() {
            return Ok(None);
        }

        // safe to unwrap obviously
        if let Err(err) = res.unwrap() {
            return Err(err);
        }

        // safe to unwrap obviously obviously :)
        let event = res.unwrap().unwrap();

        // The following part executes when an cm_event occurs
        Some(self.handle_connect_request(event)).transpose()
    }

    pub(crate) async fn accept(
        &mut self,
        cmid_handle: Handle,
        conn_param: Option<&interface::ConnParam>,
    ) -> Result<()> {
        trace!(
            "Accept, cmid_handle: {:?}, conn_param: {:?}",
            cmid_handle,
            conn_param
        );

        let cmid = self.state.resource().cmid_table.get(&cmid_handle)?;
        cmid.accept(self.get_conn_param(conn_param).as_ref())
            .map_err(ApiError::RdmaCm)?;

        // wait until the accept is done
        let event_type = rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_ESTABLISHED;
        let _event = self.wait_cm_event(&cmid_handle, event_type).await?;

        Ok(())
    }

    pub(crate) async fn connect(
        &mut self,
        cmid_handle: Handle,
        conn_param: Option<&interface::ConnParam>,
    ) -> Result<()> {
        trace!(
            "Connect, cmid_handle: {:?}, conn_param: {:?}",
            cmid_handle,
            conn_param
        );

        let cmid = self.state.resource().cmid_table.get(&cmid_handle)?;
        cmid.connect(self.get_conn_param(conn_param).as_ref())
            .map_err(ApiError::RdmaCm)?;

        // wait until the accept is done
        let event_type = rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_ESTABLISHED;
        let _event = self.wait_cm_event(&cmid_handle, event_type).await?;

        Ok(())
    }

    pub(crate) fn bind_addr(&mut self, cmid_handle: Handle, sockaddr: &SocketAddr) -> Result<()> {
        trace!(
            "BindAddr, cmid_handle: {:?}, sockaddr: {:?}",
            cmid_handle,
            sockaddr
        );

        let cmid = self.state.resource().cmid_table.get(&cmid_handle)?;
        cmid.bind_addr(sockaddr).map_err(ApiError::RdmaCm)?;
        Ok(())
    }

    pub(crate) async fn resolve_addr(
        &mut self,
        cmid_handle: Handle,
        sockaddr: &SocketAddr,
    ) -> Result<()> {
        trace!(
            "ResolveAddr: cmid_handle: {:?}, sockaddr: {:?}",
            cmid_handle,
            sockaddr
        );

        let cmid = self.state.resource().cmid_table.get(&cmid_handle)?;
        cmid.resolve_addr(sockaddr).map_err(ApiError::RdmaCm)?;

        let event_type = rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_ADDR_RESOLVED;
        let _event = self.wait_cm_event(&cmid_handle, event_type).await?;

        Ok(())
    }

    pub(crate) async fn resolve_route(
        &mut self,
        cmid_handle: Handle,
        timeout_ms: i32,
    ) -> Result<()> {
        trace!(
            "ResolveRoute: cmid_handle: {:?}, timeout_ms: {:?}",
            cmid_handle,
            timeout_ms
        );

        let cmid = self.state.resource().cmid_table.get(&cmid_handle)?;
        cmid.resolve_route(timeout_ms).map_err(ApiError::RdmaCm)?;

        let event_type = rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_ROUTE_RESOLVED;
        let _event = self.wait_cm_event(&cmid_handle, event_type).await?;

        Ok(())
    }

    pub(crate) fn cm_create_qp(
        &mut self,
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

        let cmid = self.state.resource().cmid_table.get(&cmid_handle)?;

        let pd = pd.cloned().or_else(|| {
            // use the default pd of the corresponding device
            let sgid = cmid.sgid();
            Some(
                self.state
                    .resource()
                    .default_pd(&sgid)
                    .expect("Something is wrong"),
            )
        });

        let (pd, qp_init_attr) = self.get_qp_params(pd.as_ref(), Some(qp_init_attr))?;
        let qp = cmid
            .create_qp(pd.as_deref(), qp_init_attr.as_ref())
            .map_err(ApiError::RdmaCm)?;
        let handles = self.state.resource().insert_qp(qp)?;
        Ok(prepare_returned_qp(handles))
    }

    // NOTE(cjr): reg_mr does not insert the MR to its table.
    pub(crate) fn reg_mr(
        &mut self,
        pd: &interface::ProtectionDomain,
        nbytes: usize,
        access: interface::AccessFlags,
    ) -> Result<rdma::mr::MemoryRegion> {
        trace!(
            "RegMr, pd: {:?}, nbytes: {}, access: {:?}",
            pd,
            nbytes,
            access
        );

        let pd = self.state.resource().pd_table.get(&pd.0)?;
        let mr = rdma::mr::MemoryRegion::new(&pd, nbytes, access)
            .map_err(ApiError::MemoryRegion)
            .expect("something is wrong; remove this expect() later");
        Ok(mr)
    }

    // NOTE(cjr): There is no API for dereg_mr. All the user needs to do is to drop it.

    pub(crate) fn create_cq(
        &mut self,
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
        self.state
            .resource()
            .cq_table
            .occupy_or_create_resource(handle, cq);

        Ok(returned::CompletionQueue {
            handle: interface::CompletionQueue(handle),
        })
    }

    pub(crate) fn dealloc_pd(&mut self, pd: &interface::ProtectionDomain) -> Result<()> {
        trace!("DeallocPd, pd: {:?}", pd);
        self.state.resource().pd_table.close_resource(&pd.0)?;
        Ok(())
    }

    pub(crate) fn destroy_cq(&mut self, cq: &interface::CompletionQueue) -> Result<()> {
        trace!("DestroyCq, cq: {:?}", cq);
        self.state.resource().cq_table.close_resource(&cq.0)?;
        Ok(())
    }

    pub(crate) fn destroy_qp(&mut self, qp: &interface::QueuePair) -> Result<()> {
        trace!("DestroyQp, qp: {:?}", qp);
        self.state.resource().qp_table.close_resource(&qp.0)?;
        Ok(())
    }

    pub(crate) async fn disconnect(&mut self, cmid: &interface::CmId) -> Result<()> {
        trace!("Disconnect, cmid: {:?}", cmid);

        let cmid_handle = cmid.0;
        let cmid = self.state.resource().cmid_table.get(&cmid_handle)?;
        cmid.disconnect().map_err(ApiError::RdmaCm)?;

        let event_type = rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_DISCONNECTED;
        let _event = self.wait_cm_event(&cmid_handle, event_type).await?;

        Ok(())
    }

    pub(crate) fn destroy_id(&mut self, cmid: &interface::CmId) -> Result<()> {
        trace!("DestroyId, cmid: {:?}", cmid);
        self.state.resource().cmid_table.close_resource(&cmid.0)?;
        Ok(())
    }

    pub(crate) fn open_pd(&mut self, pd: &interface::ProtectionDomain) -> Result<()> {
        trace!("OpenPd, pd: {:?}", pd);
        self.state.resource().pd_table.open_resource(&pd.0)?;
        Ok(())
    }

    pub(crate) fn open_cq(&mut self, cq: &interface::CompletionQueue) -> Result<u32> {
        trace!("OpenCq, cq: {:?}", cq);
        self.state.resource().cq_table.open_resource(&cq.0)?;
        let cq = self.state.resource().cq_table.get(&cq.0)?;
        Ok(cq.capacity())
    }

    pub(crate) fn open_qp(&mut self, qp: &interface::QueuePair) -> Result<()> {
        trace!("OpenQp, qp: {:?}", qp);
        self.state.resource().qp_table.open_resource(&qp.0)?;
        Ok(())
    }

    pub(crate) fn get_default_pds(&self) -> Result<Vec<returned::ProtectionDomain>> {
        trace!("GetDefaultPds");
        let pds = self
            .state
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

    fn get_qp_params(
        &self,
        pd_handle: Option<&interface::ProtectionDomain>,
        qp_init_attr: Option<&interface::QpInitAttr>,
    ) -> Result<(
        Option<Arc<ibv::ProtectionDomain<'static>>>,
        Option<rdma::ffi::ibv_qp_init_attr>,
    )> {
        let pd = if let Some(h) = pd_handle {
            Some(self.state.resource().pd_table.get(&h.0)?)
        } else {
            None
        };
        let qp_init_attr = if let Some(a) = qp_init_attr {
            let send_cq = if let Some(ref h) = a.send_cq {
                Some(self.state.resource().cq_table.get(&h.0)?)
            } else {
                None
            };
            let recv_cq = if let Some(ref h) = a.recv_cq {
                Some(self.state.resource().cq_table.get(&h.0)?)
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
        let fut = self.state
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
            .and_then(|manager| manager.get_one_cm_event(event_channel_handle, event_type))
    }

    fn pop_first_cm_error(&self) -> Option<ApiError> {
        self.state
            .shared
            .cm_manager
            .try_lock()
            .ok()
            .and_then(|manager| manager.first_error())
    }

    fn try_get_cm_event(
        &self,
        event_channel_handle: &Handle,
        event_type: rdma::ffi::rdma_cm_event_type::Type,
    ) -> Option<Result<rdmacm::CmEvent>> {
        if let Some(cm_event) = self.get_one_cm_event(event_channel_handle, event_type) {
            return Some(Ok(cm_event));
        }
        if let Some(err) = self.pop_first_cm_error() {
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
