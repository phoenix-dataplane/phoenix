use std::collections::VecDeque;
use std::io;
use std::mem;
use std::os::unix::io::AsRawFd;
use std::pin::Pin;
use std::slice;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;

use phoenix_api::engine::SchedulingMode;
use phoenix_api::net;
use phoenix_api::net::returned;
use phoenix_api::transport::rdma::{cmd, dp};
use phoenix_api::{AsHandle, Handle};

// use rdma::ibv;
use rdma::rdmacm;

use super::module::CustomerType;
use super::ops::Ops;
use super::{ApiError, DatapathError, Error};

use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::module::{ModuleCollection, Version};
use phoenix_common::storage::{ResourceCollection, SharedStorage};
use phoenix_common::{log, tracing};

pub(crate) struct TransportEngine {
    pub(crate) customer: CustomerType,
    pub(crate) indicator: Indicator,
    pub(crate) _mode: SchedulingMode,

    pub(crate) node: DataPathNode,
    pub(crate) ops: Ops,
    pub(crate) cq_err_buffer: VecDeque<dp::Completion>, // TODO(cjr): limit the length of the queue
    pub(crate) wr_read_buffer: Vec<dp::WorkRequest>,
}

impl_vertex_for_engine!(TransportEngine, node);

impl Decompose for TransportEngine {
    #[inline]
    fn flush(&mut self) -> Result<usize> {
        Ok(0)
    }

    fn decompose(
        self: Box<Self>,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode) {
        let engine = *self;
        let mut collections = ResourceCollection::with_capacity(5);
        tracing::trace!("dumping RdmaTransport-TransportEngine states...");
        collections.insert("customer".to_string(), Box::new(engine.customer));
        collections.insert("mode".to_string(), Box::new(engine._mode));
        collections.insert("ops".to_string(), Box::new(engine.ops));
        collections.insert("cq_err_buffer".to_string(), Box::new(engine.cq_err_buffer));
        collections.insert(
            "wr_read_buffer".to_string(),
            Box::new(engine.wr_read_buffer),
        );
        (collections, engine.node)
    }
}

impl TransportEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
        node: DataPathNode,
        _plugged: &ModuleCollection,
        _prev_version: Version,
    ) -> Result<Self> {
        tracing::trace!("restoring RdmaTransport-TransportEngine states...");
        let customer = *local
            .remove("customer")
            .unwrap()
            .downcast::<CustomerType>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let mode = *local
            .remove("mode")
            .unwrap()
            .downcast::<SchedulingMode>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let ops = *local
            .remove("ops")
            .unwrap()
            .downcast::<Ops>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let cq_err_buffer = *local
            .remove("cq_err_buffer")
            .unwrap()
            .downcast::<VecDeque<dp::Completion>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let wr_read_buffer = *local
            .remove("wr_read_buffer")
            .unwrap()
            .downcast::<Vec<dp::WorkRequest>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = TransportEngine {
            customer,
            indicator: Default::default(),
            _mode: mode,
            node,
            ops,
            cq_err_buffer,
            wr_read_buffer,
        };
        Ok(engine)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for TransportEngine {
    fn description(self: Pin<&Self>) -> String {
        format!(
            "RDMA TransportEngine, user: {:?}",
            self.ops.state.shared.pid
        )
    }

    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }
}

impl TransportEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut nwork = 0;

            loop {
                if let Progress(n) = self.check_dp(false)? {
                    nwork += n;
                    if n == 0 {
                        break;
                    }
                }
            }

            if let Status::Disconnected = self.check_cmd().await? {
                return Ok(());
            }

            self.indicator.set_nwork(nwork);
            future::yield_now().await;
        }
    }
}

impl TransportEngine {
    fn flush_dp(&mut self) -> Result<Status, DatapathError> {
        let mut processed = 0;
        let existing_work = self.customer.get_avail_wr_count()?;

        while processed < existing_work {
            if let Progress(n) = self.check_dp(true)? {
                processed += n;
            }
        }

        Ok(Progress(processed))
    }

    fn check_dp(&mut self, flushing: bool) -> Result<Status, DatapathError> {
        use dp::WorkRequest;
        let buffer_cap = self.wr_read_buffer.capacity();

        // Fetch available work requests. Copy them into a buffer.
        let max_count = if !flushing {
            buffer_cap.min(self.customer.get_avail_wc_slots()?)
        } else {
            buffer_cap
        };
        if max_count == 0 {
            return Ok(Progress(0));
        }

        let mut count = 0;
        // SAFETY: dp::WorkRequest is Copy and zerocopy
        unsafe {
            self.wr_read_buffer.set_len(0);
        }

        self.customer
            .dequeue_wr_with(|ptr, read_count| unsafe {
                // TODO(cjr): One optimization is to post all available send requests in one batch
                // using doorbell
                debug_assert!(max_count <= buffer_cap);
                count = max_count.min(read_count);
                for i in 0..count {
                    self.wr_read_buffer
                        .push(ptr.add(i).cast::<WorkRequest>().read());
                }
                count
            })
            .unwrap_or_else(|e| panic!("check_dp: {}", e));

        // Process the work requests.
        let buffer = mem::take(&mut self.wr_read_buffer);

        for wr in &buffer {
            let result = self.process_dp(wr, flushing);
            match result {
                Ok(()) => {}
                Err(e) => {
                    // NOTE(cjr): Typically, we expect to report the error to the user right after
                    // we get this error. But busy waiting here may cause circular waiting between
                    // phoenix engine and the user in some circumstance.
                    //
                    // The work queue and completion queue are both bounded. The bound is set by
                    // phoenix system rather than specified by the user. Imagine that the user is
                    // trying to post_send without polling for completion timely. The completion
                    // queue is full and phoenix will spin here without making any progress (e.g.
                    // drain the work queue).
                    //
                    // Therefore, we put the error into a local buffer. Whenever we want to put
                    // stuff in the shared memory completion queue, we put from the local buffer
                    // first. This way seems perfect. It can guarantee progress. The backpressure
                    // is also not broken.
                    let _sent = self.process_dp_error(wr, e).unwrap();
                }
            }
        }

        self.wr_read_buffer = buffer;

        self.try_flush_cq_err_buffer().unwrap();

        Ok(Progress(count))
    }

    async fn check_cmd(&mut self) -> Result<Status, Error> {
        let ret = self.customer.try_recv_cmd();
        match ret {
            // handle request
            Ok(req) => {
                // Flush datapath!
                self.flush_dp()?;
                let result = self.process_cmd(&req).await;
                match result {
                    Ok(res) => self.customer.send_comp(cmd::Completion(Ok(res)))?,
                    Err(e) => {
                        // better to log the error here, in case sometimes the customer does
                        // not receive the error
                        log::error!("process_cmd error: {}", e);
                        self.customer.send_comp(cmd::Completion(Err(e.into())))?
                    }
                }
                Ok(Progress(1))
            }
            Err(ipc::TryRecvError::Empty) => {
                // do nothing
                Ok(Progress(0))
            }
            Err(ipc::TryRecvError::Disconnected) => Ok(Status::Disconnected),
            Err(ipc::TryRecvError::Other(_e)) => Err(Error::IpcTryRecv),
        }
    }

    #[allow(dead_code)]
    fn get_event_type(req: &cmd::Command) -> rdma::ffi::rdma_cm_event_type::Type {
        use cmd::Command;
        use rdma::ffi::rdma_cm_event_type::*;
        match req {
            Command::ResolveAddr(..) => RDMA_CM_EVENT_ADDR_RESOLVED,
            Command::ResolveRoute(..) => RDMA_CM_EVENT_ROUTE_RESOLVED,
            Command::Connect(..) => RDMA_CM_EVENT_ESTABLISHED,
            Command::GetRequest(..) => RDMA_CM_EVENT_CONNECT_REQUEST,
            Command::TryGetRequest(..) => RDMA_CM_EVENT_CONNECT_REQUEST,
            Command::Accept(..) => RDMA_CM_EVENT_ESTABLISHED,
            Command::Disconnect(..) => RDMA_CM_EVENT_DISCONNECTED,
            _ => panic!("Unexpected CM type: {:?}", req),
        }
    }

    /// Return the cq_handle and wr_id for the work request
    fn get_dp_error_info(&self, wr: &dp::WorkRequest) -> (net::CompletionQueue, u64) {
        use dp::WorkRequest;
        match wr {
            WorkRequest::PostSend(cmid_handle, wr_id, ..)
            | WorkRequest::PostSendWithImm(cmid_handle, wr_id, ..) => {
                // if the cq_handle does not exists at all, set it to
                // Handle::INVALID.
                if let Ok(cmid) = self
                    .ops
                    .resource()
                    .cmid_table
                    .get_dp(cmid_handle.0 as usize)
                {
                    if let Some(qp) = cmid.qp() {
                        (net::CompletionQueue(qp.send_cq().as_handle()), *wr_id)
                    } else {
                        (net::CompletionQueue(Handle::INVALID), *wr_id)
                    }
                } else {
                    (net::CompletionQueue(Handle::INVALID), *wr_id)
                }
            }
            WorkRequest::PostRecv(cmid_handle, wr_id, ..) => {
                if let Ok(cmid) = self
                    .ops
                    .resource()
                    .cmid_table
                    .get_dp(cmid_handle.0 as usize)
                {
                    if let Some(qp) = cmid.qp() {
                        (net::CompletionQueue(qp.recv_cq().as_handle()), *wr_id)
                    } else {
                        (net::CompletionQueue(Handle::INVALID), *wr_id)
                    }
                } else {
                    (net::CompletionQueue(Handle::INVALID), *wr_id)
                }
            }
            WorkRequest::PollCq(cq_handle) => (*cq_handle, 0),
            WorkRequest::PostWrite(cmid_handle, _, wr_id, ..) => {
                if let Ok(cmid) = self
                    .ops
                    .resource()
                    .cmid_table
                    .get_dp(cmid_handle.0 as usize)
                {
                    if let Some(qp) = cmid.qp() {
                        (net::CompletionQueue(qp.send_cq().as_handle()), *wr_id)
                    } else {
                        (net::CompletionQueue(Handle::INVALID), *wr_id)
                    }
                } else {
                    (net::CompletionQueue(Handle::INVALID), *wr_id)
                }
            }
            WorkRequest::PostRead(cmid_handle, _, wr_id, ..) => {
                if let Ok(cmid) = self
                    .ops
                    .resource()
                    .cmid_table
                    .get_dp(cmid_handle.0 as usize)
                {
                    if let Some(qp) = cmid.qp() {
                        (net::CompletionQueue(qp.send_cq().as_handle()), *wr_id)
                    } else {
                        (net::CompletionQueue(Handle::INVALID), *wr_id)
                    }
                } else {
                    (net::CompletionQueue(Handle::INVALID), *wr_id)
                }
            }
        }
    }

    fn get_completion_from_error(&self, wr: &dp::WorkRequest, e: DatapathError) -> dp::Completion {
        use net::{WcStatus, WorkCompletion};
        use rdma::ffi::ibv_wc_status;
        use std::num::NonZeroU32;

        let (cq_handle, wr_id) = self.get_dp_error_info(wr);
        dp::Completion {
            cq_handle,
            _padding: Default::default(),
            wc: WorkCompletion::new_vendor_err(
                wr_id,
                WcStatus::Error(NonZeroU32::new(ibv_wc_status::IBV_WC_GENERAL_ERR).unwrap()),
                e.into_vendor_err(),
            ),
        }
    }

    /// Return the error through the work completion. The error happened in phoenix
    /// side is considered a `vendor_err`.
    ///
    /// NOTE(cjr): There's no fundamental difference between the failure on
    /// post_send and the failure on poll_cq for the same work request.
    /// The general practice is to return the error early, but we can
    /// postpone the error returning in order to achieve asynchronous IPC.
    ///
    /// However, the completion order can be changed due to some errors
    /// happen on request posting stage.
    fn process_dp_error(
        &mut self,
        wr: &dp::WorkRequest,
        e: DatapathError,
    ) -> Result<bool, DatapathError> {
        let mut sent = false;
        let comp = self.get_completion_from_error(wr, e);
        if !self.cq_err_buffer.is_empty() {
            self.cq_err_buffer.push_back(comp);
        } else {
            self.customer.notify_wc_with(|ptr, _count| unsafe {
                // construct an WorkCompletion and set the vendor_err
                ptr.cast::<dp::Completion>().write(comp);
                sent = true;
                1
            })?;
        }

        Ok(sent)
    }

    fn poll_cq_to_backup_buffer(
        &mut self,
        cq_handle: &net::CompletionQueue,
    ) -> Result<(), DatapathError> {
        let cq = self
            .ops
            .resource()
            .cq_table
            .get_dp(cq_handle.0 .0 as usize)?;
        let mut wc: [rdma::ffi::ibv_wc; 1] = Default::default();
        loop {
            match cq.poll(&mut wc) {
                Ok(completions) if !completions.is_empty() => {
                    for w in completions {
                        self.cq_err_buffer.push_back(dp::Completion {
                            cq_handle: *cq_handle,
                            _padding: Default::default(),
                            wc: unsafe { mem::transmute_copy(w) },
                        });
                    }
                }
                Ok(_) => {
                    self.cq_err_buffer.push_back(dp::Completion {
                        cq_handle: *cq_handle,
                        _padding: Default::default(),
                        wc: net::WorkCompletion::again(),
                    });
                    break;
                }
                Err(rdma::ibv::PollCqError) => {
                    return Err(DatapathError::Ibv(io::Error::last_os_error()));
                }
            }
        }
        Ok(())
    }

    fn try_flush_cq_err_buffer(&mut self) -> Result<(), DatapathError> {
        if self.cq_err_buffer.is_empty() {
            return Ok(());
        }

        let mut cq_err_buffer = VecDeque::new();
        mem::swap(&mut cq_err_buffer, &mut self.cq_err_buffer);
        let status = self.customer.notify_wc_with(|ptr, count| unsafe {
            let mut cnt = 0;
            // construct an WorkCompletion and set the vendor_err
            for comp in cq_err_buffer.drain(..count) {
                ptr.cast::<dp::Completion>().write(comp);
                cnt += 1;
            }
            cnt
        });

        mem::swap(&mut cq_err_buffer, &mut self.cq_err_buffer);
        status?;
        Ok(())
    }

    /// Process data path operations.
    fn process_dp(&mut self, req: &dp::WorkRequest, flushing: bool) -> Result<(), DatapathError> {
        use dp::WorkRequest;
        match req {
            WorkRequest::PostRecv(cmid_handle, wr_id, range, mr_handle) => {
                let mr = self.ops.resource().mr_table.get_dp(mr_handle.0 as usize)?;
                let rdma_mr = rdmacm::MemoryRegion::from(mr.as_ref());
                unsafe {
                    self.ops.post_recv(*cmid_handle, &rdma_mr, *range, *wr_id)?;
                }
                Ok(())
            }
            WorkRequest::PostSend(cmid_handle, wr_id, range, mr_handle, send_flags) => {
                let mr = self.ops.resource().mr_table.get_dp(mr_handle.0 as usize)?;
                let rdma_mr = rdmacm::MemoryRegion::from(mr.as_ref());
                unsafe {
                    self.ops
                        .post_send(*cmid_handle, &rdma_mr, *range, *wr_id, *send_flags)?;
                }
                Ok(())
            }
            WorkRequest::PostSendWithImm(cmid_handle, wr_id, range, mr_handle, send_flags, imm) => {
                let mr = self.ops.resource().mr_table.get_dp(mr_handle.0 as usize)?;
                let rdma_mr = rdmacm::MemoryRegion::from(mr.as_ref());
                unsafe {
                    self.ops.post_send_with_imm(
                        *cmid_handle,
                        &rdma_mr,
                        *range,
                        *wr_id,
                        *send_flags,
                        *imm,
                    )?;
                }
                Ok(())
            }
            WorkRequest::PostWrite(
                cmid_handle,
                mr_handle,
                wr_id,
                range,
                remote_offset,
                rkey,
                send_flags,
            ) => {
                let mr = self.ops.resource().mr_table.get_dp(mr_handle.0 as usize)?;
                let rdma_mr = rdmacm::MemoryRegion::from(mr.as_ref());
                unsafe {
                    self.ops.post_write(
                        *cmid_handle,
                        &rdma_mr,
                        *range,
                        *wr_id,
                        *rkey,
                        *remote_offset,
                        *send_flags,
                    )?;
                }
                Ok(())
            }
            WorkRequest::PostRead(
                cmid_handle,
                mr_handle,
                wr_id,
                range,
                remote_offset,
                rkey,
                send_flags,
            ) => {
                let mr = self.ops.resource().mr_table.get_dp(mr_handle.0 as usize)?;
                let rdma_mr = rdmacm::MemoryRegion::from(mr.as_ref());
                unsafe {
                    self.ops.post_read(
                        *cmid_handle,
                        &rdma_mr,
                        *range,
                        *wr_id,
                        *rkey,
                        *remote_offset,
                        *send_flags,
                    )?;
                }
                Ok(())
            }
            WorkRequest::PollCq(cq_handle) => {
                // trace!("cq_handle: {:?}", cq_handle);
                self.try_flush_cq_err_buffer()?;

                // Poll the completions and put them directly into the shared memory queue.
                //
                // NOTE(cjr): The correctness of the following code extremely depends on the memory
                // layout. It must be carefully checked.
                //
                // This send must be successful because the phoenix_syscalls uses an outstanding flag to
                // reduce the busy polling from the user appliation. If the shared memory cq is
                // full of completions from cq A, and the shared memory wq only has one poll_cq,
                // and the poll_cq is not really executed because the shmcq is full. Then
                // the outstanding flag will never be flipped and that user cq is thus dead.
                //
                // This while loop will not go forever only when we have a guard that checks the
                // write count of dp_cq is non-zero at the entry of check_dp()
                let mut err = false;
                let mut sent = false;
                while !sent {
                    if flushing {
                        let write_count = self.customer.get_avail_wc_slots()?;
                        if write_count == 0 {
                            // unlikely
                            self.poll_cq_to_backup_buffer(cq_handle)?;
                        }
                    }
                    let cq = self
                        .ops
                        .resource()
                        .cq_table
                        .get_dp(cq_handle.0 .0 as usize)?;
                    self.customer.enqueue_wc_with(|ptr, count| unsafe {
                        sent = true;
                        let mut cnt = 0;
                        while cnt < count {
                            // NOTE(cjr): For now, we can only poll 1 entry at a time, because the size
                            // of an ibv_wc is 48 bytes, however, the slot for each completion is
                            // 64-byte wide.
                            //
                            // In the future, we can embed cq_handle in the unused field in
                            // WorkCompletion, or maybe just use wr_id to find the corresponding CQ.
                            // In these ways, the CQ can be polled in batch.
                            let handle_ptr: *mut net::CompletionQueue = ptr.add(cnt).cast();
                            handle_ptr.write(*cq_handle);
                            let wc = slice::from_raw_parts_mut(
                                memoffset::raw_field!(handle_ptr, dp::Completion, wc) as _,
                                1,
                            );
                            match cq.poll(wc) {
                                Ok(completions) if !completions.is_empty() => cnt += 1,
                                Ok(_) => {
                                    wc.as_mut_ptr()
                                        .cast::<net::WorkCompletion>()
                                        .write(net::WorkCompletion::again());
                                    cnt += 1;
                                    break;
                                }
                                Err(rdma::ibv::PollCqError) => {
                                    err = true;
                                    break;
                                }
                            }
                        }
                        cnt
                    })?;
                }

                assert!(sent, "PollCq must write something to the queue");
                if err {
                    return Err(DatapathError::Ibv(io::Error::last_os_error()));
                }
                Ok(())
            }
        }
    }

    /// Process control path operations.
    async fn process_cmd(&mut self, req: &cmd::Command) -> Result<cmd::CompletionKind, Error> {
        use cmd::{Command, CompletionKind};
        match req {
            Command::GetAddrInfo(node, service, hints) => {
                let ai =
                    self.ops
                        .get_addr_info(node.as_deref(), service.as_deref(), hints.as_ref())?;
                Ok(CompletionKind::GetAddrInfo(ai))
            }
            Command::CreateEp(ai, pd, qp_init_attr) => {
                let ret_cmid = self.ops.create_ep(ai, pd.as_ref(), qp_init_attr.as_ref())?;
                Ok(CompletionKind::CreateEp(ret_cmid))
            }
            Command::CreateId(port_space) => {
                let ret_cmid = self.ops.create_id(*port_space)?;
                Ok(CompletionKind::CreateId(ret_cmid))
            }
            Command::Listen(cmid_handle, backlog) => {
                self.ops.listen(*cmid_handle, *backlog)?;
                Ok(CompletionKind::Listen)
            }
            Command::GetRequest(listener_handle) => {
                let ret_cmid = self.ops.get_request(*listener_handle).await?;
                Ok(CompletionKind::GetRequest(ret_cmid))
            }
            Command::TryGetRequest(listener_handle) => {
                let ret_cmid = self.ops.try_get_request(*listener_handle)?;
                Ok(CompletionKind::TryGetRequest(ret_cmid))
            }
            Command::Accept(cmid_handle, conn_param) => {
                self.ops.accept(*cmid_handle, conn_param.as_ref()).await?;
                Ok(CompletionKind::Accept)
            }
            Command::Connect(cmid_handle, conn_param) => {
                self.ops.connect(*cmid_handle, conn_param.as_ref()).await?;
                Ok(CompletionKind::Connect)
            }
            Command::BindAddr(cmid_handle, sockaddr) => {
                self.ops.bind_addr(*cmid_handle, sockaddr)?;
                Ok(CompletionKind::BindAddr)
            }
            Command::ResolveAddr(cmid_handle, sockaddr) => {
                self.ops.resolve_addr(*cmid_handle, sockaddr).await?;
                Ok(CompletionKind::ResolveAddr)
            }
            Command::ResolveRoute(cmid_handle, timeout_ms) => {
                self.ops.resolve_route(*cmid_handle, *timeout_ms).await?;
                Ok(CompletionKind::ResolveRoute)
            }
            Command::CmCreateQp(cmid_handle, pd, qp_init_attr) => {
                let ret_qp = self
                    .ops
                    .cm_create_qp(*cmid_handle, pd.as_ref(), qp_init_attr)?;
                Ok(CompletionKind::CmCreateQp(ret_qp))
            }
            Command::RegMr(pd, nbytes, access) => {
                let mr = self.ops.reg_mr(pd, *nbytes, *access)?;

                // TODO(cjr): If there is an error above, the customer will be confused.
                // Because the customer blocks at recv_fd.
                // The customer will never know there is an error actually happens
                let fd = mr.memfd().as_raw_fd();
                self.customer
                    .send_fd(&[fd][..])
                    .map_err(ApiError::SendFd)
                    .unwrap();

                let vaddr = mr.as_ptr().addr() as _;
                let rkey = mr.rkey();
                let raw_mr_handle = mr.as_handle();
                let file_off = mr.file_off() as u64;
                let pd_handle = mr.pd().as_handle();
                let key = self
                    .ops
                    .resource()
                    .mr_table
                    .occupy_or_create_resource(raw_mr_handle, mr)
                    .map_err(ApiError::from)?;
                let new_mr_handle = Handle(key as u64);

                let ret_mr = returned::MemoryRegion {
                    handle: net::MemoryRegion(new_mr_handle),
                    rkey,
                    vaddr,
                    map_len: *nbytes as u64,
                    file_off,
                    pd: net::ProtectionDomain(pd_handle),
                };

                Ok(CompletionKind::RegMr(ret_mr))
            }
            Command::DeregMr(mr) => {
                tracing::trace!("DeregMr, mr: {:?}", mr);
                self.ops
                    .resource()
                    .mr_table
                    .close_resource_by_key(mr.0 .0 as usize)
                    .map_err(ApiError::from)?;
                Ok(CompletionKind::DeregMr)
            }
            Command::DeallocPd(pd) => {
                self.ops.dealloc_pd(pd)?;
                Ok(CompletionKind::DeallocPd)
            }
            Command::DestroyCq(cq) => {
                self.ops.destroy_cq(cq)?;
                Ok(CompletionKind::DestroyCq)
            }
            Command::DestroyQp(qp) => {
                self.ops.destroy_qp(qp)?;
                Ok(CompletionKind::DestroyQp)
            }
            Command::Disconnect(cmid) => {
                self.ops.disconnect(cmid)?;
                Ok(CompletionKind::Disconnect)
            }
            Command::DestroyId(cmid) => {
                self.ops.destroy_id(cmid)?;
                Ok(CompletionKind::DestroyId)
            }

            Command::OpenPd(pd) => {
                self.ops.open_pd(pd)?;
                Ok(CompletionKind::OpenPd)
            }
            Command::OpenCq(cq) => {
                let cq_cap = self.ops.open_cq(cq)?;
                Ok(CompletionKind::OpenCq(cq_cap))
            }
            Command::OpenQp(qp) => {
                self.ops.open_qp(qp)?;
                Ok(CompletionKind::OpenQp)
            }
            Command::GetDefaultPds => {
                let pds = self.ops.get_default_pds()?;
                Ok(CompletionKind::GetDefaultPds(pds))
            }
            Command::GetDefaultContexts => {
                let ctx_list = self.ops.get_default_contexts()?;
                Ok(CompletionKind::GetDefaultContexts(ctx_list))
            }
            Command::CreateCq(ctx, min_cq_entries, cq_context) => {
                let ret_cq = self.ops.create_cq(ctx, *min_cq_entries, *cq_context)?;
                Ok(CompletionKind::CreateCq(ret_cq))
            }
        }
    }
}
