use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::mem;
use std::os::unix::io::AsRawFd;
use std::slice;

use interface::engine::SchedulingMode;
use interface::{returned, AsHandle, Handle};
use ipc::transport::rdma::{cmd, dp};

use rdma::ibv;
use rdma::rdmacm;

use super::module::CustomerType;
use super::ops::Ops;
use super::{ApiError, DatapathError, Error};
use crate::engine::{future, Engine, EngineResult, Indicator};
use crate::node::Node;

pub(crate) struct TransportEngine {
    pub(crate) customer: CustomerType,
    pub(crate) node: Node,
    pub(crate) indicator: Option<Indicator>,
    pub(crate) _mode: SchedulingMode,

    pub(crate) ops: Ops,
    pub(crate) cq_err_buffer: VecDeque<dp::Completion>, // TODO(cjr): limit the length of the queue
    pub(crate) wr_read_buffer: Vec<dp::WorkRequest>,
}

crate::unimplemented_ungradable!(TransportEngine);
crate::impl_vertex_for_engine!(TransportEngine, node);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for TransportEngine {
    type Future = impl Future<Output = EngineResult> + 'static;

    fn description(&self) -> String {
        format!(
            "RDMA TransportEngine, user pid: {:?}",
            self.ops.state.shared.pid
        )
    }

    fn set_tracker(&mut self, indicator: Indicator) {
        assert!(
            self.indicator.replace(indicator).is_none(),
            "already has a progress tracker"
        );
    }

    fn entry(mut self) -> Self::Future {
        Box::pin(async move { self.mainloop().await })
    }
}

impl TransportEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut nwork = 0;
            if let Progress(n) = self.check_dp(false)? {
                nwork += n;
            }

            if let Status::Disconnected = self.check_cmd().await? {
                return Ok(());
            }

            self.indicator.as_ref().unwrap().set_nwork(nwork);
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

        // TODO(cjr): flamegraph shows that a large portion of time is spent in this with_capacity
        // optimize this with smallvec or so.
        let mut count = 0;
        self.wr_read_buffer.clear();

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
                    // koala engine and the user in some circumstance.
                    //
                    // The work queue and completion queue are both bounded. The bound is set by
                    // koala system rather than specified by the user. Imagine that the user is
                    // trying to post_send without polling for completion timely. The completion
                    // queue is full and koala will spin here without making any progress (e.g.
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
    fn get_dp_error_info(&self, wr: &dp::WorkRequest) -> (interface::CompletionQueue, u64) {
        use dp::WorkRequest;
        match wr {
            WorkRequest::PostSend(cmid_handle, wr_id, ..)
            | WorkRequest::PostSendWithImm(cmid_handle, wr_id, ..) => {
                // if the cq_handle does not exists at all, set it to
                // Handle::INVALID.
                if let Ok(cmid) = self.ops.resource().cmid_table.get_dp(cmid_handle) {
                    if let Some(qp) = cmid.qp() {
                        (interface::CompletionQueue(qp.send_cq().as_handle()), *wr_id)
                    } else {
                        (interface::CompletionQueue(Handle::INVALID), *wr_id)
                    }
                } else {
                    (interface::CompletionQueue(Handle::INVALID), *wr_id)
                }
            }
            WorkRequest::PostRecv(cmid_handle, wr_id, ..) => {
                if let Ok(cmid) = self.ops.resource().cmid_table.get_dp(cmid_handle) {
                    if let Some(qp) = cmid.qp() {
                        (interface::CompletionQueue(qp.recv_cq().as_handle()), *wr_id)
                    } else {
                        (interface::CompletionQueue(Handle::INVALID), *wr_id)
                    }
                } else {
                    (interface::CompletionQueue(Handle::INVALID), *wr_id)
                }
            }
            WorkRequest::PollCq(cq_handle) => (*cq_handle, 0),
            WorkRequest::PostWrite(cmid_handle, _, wr_id, ..) => {
                if let Ok(cmid) = self.ops.resource().cmid_table.get_dp(cmid_handle) {
                    if let Some(qp) = cmid.qp() {
                        (interface::CompletionQueue(qp.send_cq().as_handle()), *wr_id)
                    } else {
                        (interface::CompletionQueue(Handle::INVALID), *wr_id)
                    }
                } else {
                    (interface::CompletionQueue(Handle::INVALID), *wr_id)
                }
            }
            WorkRequest::PostRead(cmid_handle, _, wr_id, ..) => {
                if let Ok(cmid) = self.ops.resource().cmid_table.get_dp(cmid_handle) {
                    if let Some(qp) = cmid.qp() {
                        (interface::CompletionQueue(qp.send_cq().as_handle()), *wr_id)
                    } else {
                        (interface::CompletionQueue(Handle::INVALID), *wr_id)
                    }
                } else {
                    (interface::CompletionQueue(Handle::INVALID), *wr_id)
                }
            }
        }
    }

    fn get_completion_from_error(&self, wr: &dp::WorkRequest, e: DatapathError) -> dp::Completion {
        use interface::{WcStatus, WorkCompletion};
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

    /// Return the error through the work completion. The error happened in koala
    /// side is considered a `vendor_err`.
    ///
    /// NOTE(cjr): There's no fundamental difference between the failure on
    /// post_send and the failure on poll_cq for the same work request.
    /// The general practice is to return the error early, but we can
    /// postphone the error returning in order to achieve asynchronous IPC.
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
        cq_handle: &interface::CompletionQueue,
        cq: &ibv::CompletionQueue,
    ) -> Result<(), DatapathError> {
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
                        wc: interface::WorkCompletion::again(),
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
                let mr = self.ops.resource().mr_table.get_dp(mr_handle)?;
                let rdma_mr = rdmacm::MemoryRegion::from(&mr);
                unsafe {
                    self.ops.post_recv(*cmid_handle, &rdma_mr, *range, *wr_id)?;
                }
                Ok(())
            }
            WorkRequest::PostSend(cmid_handle, wr_id, range, mr_handle, send_flags) => {
                let mr = self.ops.resource().mr_table.get_dp(mr_handle)?;
                let rdma_mr = rdmacm::MemoryRegion::from(&mr);
                unsafe {
                    self.ops
                        .post_send(*cmid_handle, &rdma_mr, *range, *wr_id, *send_flags)?;
                }
                Ok(())
            }
            WorkRequest::PostSendWithImm(cmid_handle, wr_id, range, mr_handle, send_flags, imm) => {
                let mr = self.ops.resource().mr_table.get_dp(mr_handle)?;
                let rdma_mr = rdmacm::MemoryRegion::from(&mr);
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
                let mr = self.ops.resource().mr_table.get_dp(mr_handle)?;
                let rdma_mr = rdmacm::MemoryRegion::from(&mr);
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
                let mr = self.ops.resource().mr_table.get_dp(mr_handle)?;
                let rdma_mr = rdmacm::MemoryRegion::from(&mr);
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
                let cq = self.ops.resource().cq_table.get_dp(&cq_handle.0)?;

                // Poll the completions and put them directly into the shared memory queue.
                //
                // NOTE(cjr): The correctness of the following code extremely depends on the memory
                // layout. It must be carefully checked.
                //
                // This send must be successful because the libkoala uses an outstanding flag to
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
                            self.poll_cq_to_backup_buffer(cq_handle, &cq)?;
                        }
                    }
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
                            let handle_ptr: *mut interface::CompletionQueue = ptr.add(cnt).cast();
                            handle_ptr.write(*cq_handle);
                            let mut wc = slice::from_raw_parts_mut(
                                memoffset::raw_field!(handle_ptr, dp::Completion, wc) as _,
                                1,
                            );
                            match cq.poll(&mut wc) {
                                Ok(completions) if !completions.is_empty() => cnt += 1,
                                Ok(_) => {
                                    wc.as_mut_ptr()
                                        .cast::<interface::WorkCompletion>()
                                        .write(interface::WorkCompletion::again());
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
                let ret_cmid = self.ops.create_id(*port_space).await?;
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
                let new_mr_handle = mr.as_handle();
                let file_off = mr.file_off() as u64;
                let pd_handle = mr.pd().as_handle();
                self.ops
                    .resource()
                    .mr_table
                    .insert(new_mr_handle, mr)
                    .map_err(ApiError::from)?;

                let ret_mr = returned::MemoryRegion {
                    handle: interface::MemoryRegion(new_mr_handle),
                    rkey,
                    vaddr,
                    map_len: *nbytes as u64,
                    file_off,
                    pd: interface::ProtectionDomain(pd_handle),
                };

                Ok(CompletionKind::RegMr(ret_mr))
            }
            Command::DeregMr(mr) => {
                log::trace!("DeregMr, mr: {:?}", mr);
                self.ops
                    .resource()
                    .mr_table
                    .close_resource(&mr.0)
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
                self.ops.disconnect(cmid).await?;
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
