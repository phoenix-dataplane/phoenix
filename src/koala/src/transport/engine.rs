use std::collections::VecDeque;
use std::io;
use std::mem;
use std::mem::ManuallyDrop;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::slice;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use interface::{returned, Handle};
use ipc::unix::DomainSocket;
use ipc::{self, cmd, dp};

use engine::{Engine, EngineStatus, SchedulingMode, Upgradable, Version};

use rdma::ibv;
use rdma::rdmacm;
use rdma::rdmacm::CmId;

use super::state::State;
use super::{DatapathError, Error};

pub struct TransportEngine<'ctx> {
    /// This is the path of the domain socket which is client side is listening on.
    /// The mainly purpose of keeping is to send file descriptors to the client.
    pub(crate) client_path: PathBuf,
    pub(crate) sock: DomainSocket,
    pub(crate) cmd_rx_entries: ipc::ShmObject<AtomicUsize>,
    pub(crate) cmd_tx: ipc::IpcSender<cmd::Response>,
    pub(crate) cmd_rx: ipc::IpcReceiver<cmd::Request>,
    pub(crate) dp_wq: ipc::ShmReceiver<dp::WorkRequestSlot>,
    pub(crate) dp_cq: ipc::ShmSender<dp::CompletionSlot>,
    pub(crate) cq_err_buffer: VecDeque<dp::Completion>,

    pub(crate) dp_spin_cnt: usize,
    pub(crate) backoff: usize,
    pub(crate) _mode: SchedulingMode,

    pub(crate) state: State<'ctx>,
    // bufferred control path request
    pub(crate) cmd_buffer: Option<cmd::Request>,
    // otherwise, the
    pub(crate) last_cmd_ts: Instant,
}

impl<'ctx> Upgradable for TransportEngine<'ctx> {
    fn version(&self) -> Version {
        unimplemented!();
    }

    fn check_compatible(&self, _v2: Version) -> bool {
        unimplemented!();
    }

    fn suspend(&mut self) {
        unimplemented!();
    }

    fn dump(&self) {
        unimplemented!();
    }

    fn restore(&mut self) {
        unimplemented!();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl<'ctx> Engine for TransportEngine<'ctx> {
    fn resume(&mut self) -> Result<EngineStatus, Box<dyn std::error::Error>> {
        const DP_LIMIT: usize = 1 << 17;
        const CMD_MAX_INTERVAL_MS: u64 = 1000;
        if let Progress(n) = self.check_dp()? {
            if n > 0 {
                self.backoff = DP_LIMIT.min(self.backoff * 2);
            }
        }

        self.dp_spin_cnt += 1;
        if self.dp_spin_cnt < self.backoff {
            return Ok(EngineStatus::Continue);
        }

        self.dp_spin_cnt = 0;

        if self.cmd_rx_entries.load(Ordering::Relaxed) > 0
            || self.last_cmd_ts.elapsed() > Duration::from_millis(CMD_MAX_INTERVAL_MS)
        {
            self.last_cmd_ts = Instant::now();
            self.backoff = std::cmp::max(1, self.backoff / 2);
            self.flush_dp()?;
            if let Status::Disconnected = self.check_cmd()? {
                return Ok(EngineStatus::Complete);
            }
        } else {
            self.backoff = DP_LIMIT.min(self.backoff * 2);
        }

        if self.cmd_buffer.is_some() {
            self.check_cm_event()?;
        }

        Ok(EngineStatus::Continue)
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

impl<'ctx> TransportEngine<'ctx> {
    fn flush_dp(&mut self) -> Result<Status, DatapathError> {
        let mut processed = 0;
        let existing_work = self.dp_wq.receiver_mut().read_count()?;

        while processed < existing_work {
            if let Progress(n) = self.check_dp()? {
                processed += n;
            }
        }

        Ok(Progress(processed))
    }

    fn check_dp(&mut self) -> Result<Status, DatapathError> {
        use dp::WorkRequest;
        const BUF_LEN: usize = 32;

        // Fetch available work requests. Copy them into a buffer.
        let max_count = BUF_LEN.min(self.dp_cq.sender_mut().write_count()?);
        if max_count == 0 {
            return Ok(Progress(0));
        }

        let mut count = 0;
        let mut buffer = Vec::with_capacity(BUF_LEN);

        self.dp_wq
            .receiver_mut()
            .recv(|ptr, read_count| unsafe {
                // TODO(cjr): One optimization is to post all available send requests in one batch
                // using doorbell
                debug_assert!(max_count <= BUF_LEN);
                count = max_count.min(read_count);
                for i in 0..count {
                    buffer.push(ptr.add(i).cast::<WorkRequest>().read());
                }
                count
            })
            .unwrap_or_else(|e| panic!("check_dp: {}", e));

        // Process the work requests.
        for wr in &buffer {
            let result = self.process_dp(wr);
            match result {
                Ok(()) => {}
                Err(e) => {
                    // NOTE(cjr): Typically, we expect to report the error to the user right after
                    // we get this error. But busy waiting here may cause circular waiting between
                    // koala engine and the user in some circumstance.
                    //
                    // The work queue and completion queue are both bounded. The bound is set by
                    // koala system rather than the specified by the user. Imagine that the user is
                    // trying to post_send without polling for completion timely. The completion
                    // queue is full and koala will spin here without make any other progress (e.g.
                    // drain the work queue).
                    //
                    // Therefore, we put the error into a local buffer. Whenever we want to put
                    // things in the shared memory completion queue, we put from the local buffer
                    // first. This way seems perfect. It can guarantee progress. The backpressure
                    // is also not broken.
                    let _sent = self.process_dp_error(wr, e).unwrap();
                }
            }
        }

        self.try_flush_cq_err_buffer().unwrap();

        Ok(Progress(count))
    }

    fn check_cmd(&mut self) -> Result<Status, Error> {
        match self.cmd_rx.try_recv() {
            // handle request
            Ok(req) => {
                self.cmd_rx_entries.fetch_sub(1, Ordering::Relaxed);
                let result = self.process_cmd(&req);
                match result {
                    Ok(res) => self.cmd_tx.send(cmd::Response(Ok(res)))?,
                    Err(Error::InProgress) => {
                        // nothing to do, waiting for some network/device response
                        return Ok(Progress(0));
                    }
                    Err(e) => self.cmd_tx.send(cmd::Response(Err(e.into())))?,
                }
                Ok(Progress(1))
            }
            Err(ipc::TryRecvError::Empty) => {
                // do nothing
                Ok(Progress(0))
            }
            Err(ipc::TryRecvError::IpcError(ipc::IpcError::Disconnected)) => {
                Ok(Status::Disconnected)
            }
            Err(ipc::TryRecvError::IpcError(_e)) => Err(Error::IpcTryRecv),
        }
    }

    fn check_cm_event(&mut self) -> Result<Status, Error> {
        match self.state.poll_cm_event_once() {
            Ok(()) => {}
            Err(Error::NoCmEvent) => {}
            Err(e @ (Error::RdmaCm(_) | Error::Mio(_))) => {
                self.cmd_tx.send(cmd::Response(Err(e.into())))?;
                return Ok(Progress(1));
            }
            Err(e) => return Err(e),
        }

        use ipc::cmd::Request;
        assert!(self.cmd_buffer.is_some());
        let req = self.cmd_buffer.as_ref().unwrap();
        let cmd_handle = match req {
            Request::ResolveAddr(h, ..) => h,
            Request::ResolveRoute(h, ..) => h,
            Request::Connect(h, ..) => h,
            Request::GetRequest(h) => h,
            Request::Accept(h, ..) => h,
            Request::Disconnect(h) => &h.0,
            _ => panic!("Unexpected CM type: {:?}", req),
        };

        let cmid = self.state.resource().cmid_table.get(cmd_handle)?;
        let event_channel = ManuallyDrop::new(cmid.event_channel());
        let event_channel_handle = event_channel.handle().into();

        match self.state.get_one_cm_event(&event_channel_handle) {
            Some(cm_event) => {
                // the event must be consumed
                match self.process_cm_event(cm_event) {
                    Ok(res) => self.cmd_tx.send(cmd::Response(Ok(res)))?,
                    Err(e) => self.cmd_tx.send(cmd::Response(Err(e.into())))?,
                }
                Ok(Progress(1))
            }
            None => {
                // try again next time
                Ok(Progress(0))
            }
        }
    }

    /// Return the cq_handle and wr_id for the work request
    fn get_dp_error_info(&self, wr: &dp::WorkRequest) -> (interface::CompletionQueue, u64) {
        use dp::WorkRequest;
        match wr {
            WorkRequest::PostSend(cmid_handle, wr_id, ..) => {
                // if the cq_handle does not exists at all, set it to
                // Handle::INVALID.
                if let Ok(cmid) = self.state.resource().cmid_table.get_dp(cmid_handle) {
                    if let Some(qp) = cmid.qp() {
                        (
                            interface::CompletionQueue(qp.send_cq().handle().into()),
                            *wr_id,
                        )
                    } else {
                        (interface::CompletionQueue(Handle::INVALID), *wr_id)
                    }
                } else {
                    (interface::CompletionQueue(Handle::INVALID), *wr_id)
                }
            }
            WorkRequest::PostRecv(cmid_handle, wr_id, ..) => {
                if let Ok(cmid) = self.state.resource().cmid_table.get_dp(cmid_handle) {
                    if let Some(qp) = cmid.qp() {
                        (
                            interface::CompletionQueue(qp.recv_cq().handle().into()),
                            *wr_id,
                        )
                    } else {
                        (interface::CompletionQueue(Handle::INVALID), *wr_id)
                    }
                } else {
                    (interface::CompletionQueue(Handle::INVALID), *wr_id)
                }
            }
            WorkRequest::PollCq(cq_handle) => (*cq_handle, 0),

            WorkRequest::PostWrite(cmid_handle, _, wr_id, ..) => {
                if let Ok(cmid) = self.state.resource().cmid_table.get_dp(cmid_handle) {
                    if let Some(qp) = cmid.qp() {
                        (
                            interface::CompletionQueue(qp.send_cq().handle().into()),
                            *wr_id,
                        )
                    } else {
                        (interface::CompletionQueue(Handle::INVALID), *wr_id)
                    }
                } else {
                    (interface::CompletionQueue(Handle::INVALID), *wr_id)
                }
            }

            WorkRequest::PostRead(cmid_handle, _, wr_id, ..) => {
                if let Ok(cmid) = self.state.resource().cmid_table.get_dp(cmid_handle) {
                    if let Some(qp) = cmid.qp() {
                        (
                            interface::CompletionQueue(qp.send_cq().handle().into()),
                            *wr_id,
                        )
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
                e.as_vendor_err(),
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
            self.dp_cq.send_raw(|ptr, _count| unsafe {
                // construct an WorkCompletion and set the vendor_err
                ptr.cast::<dp::Completion>().write(comp);
                sent = true;
                1
            })?;
        }

        Ok(sent)
    }

    fn try_flush_cq_err_buffer(&mut self) -> Result<(), DatapathError> {
        if self.cq_err_buffer.is_empty() {
            return Ok(());
        }

        let mut cq_err_buffer = VecDeque::new();
        mem::swap(&mut cq_err_buffer, &mut self.cq_err_buffer);
        let status = self.dp_cq.send_raw(|ptr, count| unsafe {
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
    fn process_dp(&mut self, req: &dp::WorkRequest) -> Result<(), DatapathError> {
        use ipc::dp::WorkRequest;
        match req {
            WorkRequest::PostRecv(cmid_handle, wr_id, range, mr_handle) => {
                // trace!(
                //     "cmid_handle: {:?}, wr_id: {:?}, range: {:x?}, mr_handle: {:?}",
                //     cmid_handle,
                //     wr_id,
                //     user_buf,
                //     mr_handle
                // );
                let cmid = self.state.resource().cmid_table.get_dp(cmid_handle)?;
                let mr = self.state.resource().mr_table.get_dp(mr_handle)?;

                unsafe {
                    // since post_recv itself is already unsafe, it is the user's responsibility to
                    // make sure the received data is valid. The user must avoid post_recv a same
                    // buffer multiple times (e.g. from a single thread or from multiple threads)
                    // without any synchronization.
                    let rdma_mr = rdmacm::MemoryRegion::from(&mr);
                    let buf = &mr[range.offset as usize..(range.offset + range.len) as usize];
                    let buf_mut = slice::from_raw_parts_mut(buf.as_ptr() as _, buf.len());
                    cmid.post_recv(*wr_id, buf_mut, &rdma_mr)
                        .map_err(DatapathError::RdmaCm)?;
                };
                Ok(())
            }
            WorkRequest::PostSend(cmid_handle, wr_id, range, mr_handle, send_flags) => {
                // trace!(
                //     "cmid_handle: {:?}, wr_id: {:?}, range: {:x?}, mr_handle: {:?}, send_flags: {:?}",
                //     cmid_handle,
                //     wr_id,
                //     user_buf,
                //     mr_handle,
                //     send_flags,
                // );
                let cmid = self.state.resource().cmid_table.get_dp(cmid_handle)?;
                let mr = self.state.resource().mr_table.get_dp(mr_handle)?;

                let rdma_mr = rdmacm::MemoryRegion::from(&mr);
                let buf = &mr[range.offset as usize..(range.offset + range.len) as usize];

                let flags: ibv::SendFlags = (*send_flags).into();
                unsafe { cmid.post_send(*wr_id, buf, &rdma_mr, flags.0) }
                    .map_err(DatapathError::RdmaCm)?;
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
                let cmid = self.state.resource().cmid_table.get_dp(cmid_handle)?;
                let mr = self.state.resource().mr_table.get_dp(mr_handle)?;

                let rdma_mr = rdmacm::MemoryRegion::from(&mr);
                let buf = &mr[range.offset as usize..(range.offset + range.len) as usize];
                let remote_addr = rkey.addr + remote_offset;

                let flags: ibv::SendFlags = (*send_flags).into();
                unsafe { cmid.post_write(*wr_id, buf, &rdma_mr, flags.0, remote_addr, rkey.rkey) }
                    .map_err(DatapathError::RdmaCm)?;

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
                let cmid = self.state.resource().cmid_table.get_dp(cmid_handle)?;
                let mr = self.state.resource().mr_table.get_dp(mr_handle)?;

                let remote_addr = rkey.addr + remote_offset;
                let flags: ibv::SendFlags = (*send_flags).into();

                unsafe {
                    let rdma_mr = rdmacm::MemoryRegion::from(&mr);
                    let buf = &mr[range.offset as usize..(range.offset + range.len) as usize];
                    let buf_mut = slice::from_raw_parts_mut(buf.as_ptr() as _, buf.len());
                    cmid.post_read(*wr_id, buf_mut, &rdma_mr, flags.0, remote_addr, rkey.rkey)
                        .map_err(DatapathError::RdmaCm)?;
                };
                Ok(())
            }
            WorkRequest::PollCq(cq_handle) => {
                // trace!("cq_handle: {:?}", cq_handle);
                self.try_flush_cq_err_buffer()?;
                let cq = self.state.resource().cq_table.get_dp(&cq_handle.0)?;

                // Poll the completions and put them directly into the shared memory queue.
                //
                // NOTE(cjr): The correctness of the following code extremely depends on the memory
                // layout. It must be carefully checked.
                //
                // This send must be successful because the libkoala uses an outstanding flag to
                // reduce the busy polling from the user appliation. If the shared memory cq is
                // full of completion from another cq, and the shared memory wq only has one
                // poll_cq, and the poll_cq is not really executed because the shmcq is full. Then
                // the outstanding flag will never be flipped and that user cq is thus dead.
                let mut err = false;
                let mut sent = false;
                while !sent {
                    self.dp_cq.sender_mut().send(|ptr, count| unsafe {
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
                                Err(()) => {
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

    fn get_qp_params(
        &self,
        pd_handle: &Option<interface::ProtectionDomain>,
        qp_init_attr: Option<&interface::QpInitAttr>,
    ) -> Result<
        (
            Option<Arc<ibv::ProtectionDomain<'ctx>>>,
            Option<rdma::ffi::ibv_qp_init_attr>,
        ),
        Error,
    > {
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
                cap: a.cap.clone().into(),
                qp_type: a.qp_type.into(),
                sq_sig_all: a.sq_sig_all,
            };
            Some(attr.to_ibv_qp_init_attr())
        } else {
            None
        };
        Ok((pd, qp_init_attr))
    }

    fn get_conn_param<'a>(
        &'a self,
        conn_param: &Option<interface::ConnParam>,
    ) -> Option<rdma::ffi::rdma_conn_param> {
        conn_param.as_ref().map(|param| rdma::ffi::rdma_conn_param {
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

    fn process_cm_event(&mut self, event: rdmacm::CmEvent) -> Result<cmd::ResponseKind, Error> {
        assert!(self.cmd_buffer.is_some());
        let req = self.cmd_buffer.take().unwrap();
        if event.status() < 0 {
            return Err(Error::RdmaCm(io::Error::from_raw_os_error(-event.status())));
        } else if event.status() > 0 {
            return Err(Error::Transport(event.status()));
        }

        use ipc::cmd::{Request, ResponseKind};
        use rdma::ffi::rdma_cm_event_type::*;
        match event.event() {
            RDMA_CM_EVENT_ADDR_RESOLVED => {
                assert!(matches!(req, Request::ResolveAddr(..)), "{:?}", req);
                Ok(ResponseKind::ResolveAddr)
            }
            RDMA_CM_EVENT_ROUTE_RESOLVED => {
                assert!(matches!(req, Request::ResolveRoute(..)), "{:?}", req);
                Ok(ResponseKind::ResolveRoute)
            }
            RDMA_CM_EVENT_CONNECT_REQUEST => match req {
                Request::GetRequest(_listener_handle) => {
                    // let listener = self.state.resource().cmid_table.get(listener_handle)?;
                    // assert_eq!(
                    //     listener_handle,
                    //     &Handle::from(event.listen_id().unwrap().handle())
                    // );
                    let (new_cmid, new_qp) = event.id_owned();

                    let ret_qp = if let Some(qp) = new_qp {
                        let handles = self.state.resource().insert_qp(qp)?;
                        Some(prepare_returned_qp(handles))
                    } else {
                        None
                    };
                    let new_cmid_handle = self.state.resource().insert_cmid(new_cmid)?;
                    let ret_cmid = returned::CmId {
                        handle: interface::CmId(new_cmid_handle),
                        qp: ret_qp,
                    };
                    Ok(ResponseKind::GetRequest(ret_cmid))
                }
                _ => {
                    panic!("Expect GetRequest, found: {:?}", req);
                }
            },
            RDMA_CM_EVENT_ESTABLISHED => match req {
                Request::Connect(_cmid_handle, ..) => {
                    // assert_eq!(cmid_handle, Handle::from(event.id().handle()));
                    Ok(ResponseKind::Connect)
                }
                Request::Accept(_cmid_handle, ..) => {
                    // assert_eq!(cmid_handle, Handle::from(event.id().handle()));
                    Ok(ResponseKind::Accept)
                }
                _ => {
                    panic!("Expect Connect/Accept, found: {:?}", req);
                }
            },
            RDMA_CM_EVENT_DISCONNECTED => {
                assert!(matches!(req, Request::Disconnect(..)), "{:?}", req);
                Ok(ResponseKind::Disconnect)
            }
            _ => {
                panic!("Unhandled event type: {}", event.event());
            }
        }
    }

    /// Process control path operations.
    fn process_cmd(&mut self, req: &cmd::Request) -> Result<cmd::ResponseKind, Error> {
        use ipc::cmd::{Request, ResponseKind};
        match req {
            Request::NewClient(..) => unreachable!(),
            &Request::Hello(number) => Ok(ResponseKind::HelloBack(number)),
            Request::GetAddrInfo(node, service, hints) => {
                trace!(
                    "GetAddrInfo, node: {:?}, service: {:?}, hints: {:?}",
                    node,
                    service,
                    hints,
                );
                let hints = hints.map(rdmacm::AddrInfoHints::from);
                let ret = rdmacm::AddrInfo::getaddrinfo(
                    node.as_deref(),
                    service.as_deref(),
                    hints.as_ref(),
                );
                match ret {
                    Ok(ai) => Ok(ResponseKind::GetAddrInfo(ai.into())),
                    Err(e) => Err(Error::GetAddrInfo(e)),
                }
            }
            Request::CreateEp(ai, pd, qp_init_attr) => {
                trace!(
                    "CreateEp, ai: {:?}, pd: {:?}, qp_init_attr: {:?}",
                    ai,
                    pd,
                    qp_init_attr
                );

                let (pd, qp_init_attr) = self.get_qp_params(pd, qp_init_attr.as_ref())?;
                match CmId::create_ep(&ai.clone().into(), pd.as_deref(), qp_init_attr.as_ref()) {
                    Ok((cmid, qp)) => {
                        let cmid_handle = self.state.resource().insert_cmid(cmid)?;
                        let ret_qp = if let Some(qp) = qp {
                            let handles = self.state.resource().insert_qp(qp)?;
                            Some(prepare_returned_qp(handles))
                        } else {
                            None
                        };
                        let ret_cmid = returned::CmId {
                            handle: interface::CmId(cmid_handle),
                            qp: ret_qp,
                        };
                        Ok(ResponseKind::CreateEp(ret_cmid))
                    }
                    Err(e) => Err(Error::RdmaCm(e)),
                }
            }
            Request::Listen(cmid_handle, backlog) => {
                trace!(
                    "Listen, cmid_handle: {:?}, backlog: {}",
                    cmid_handle,
                    backlog
                );
                let listener = self.state.resource().cmid_table.get(cmid_handle)?;
                listener.listen(*backlog).map_err(Error::RdmaCm)?;
                Ok(ResponseKind::Listen)
            }
            Request::GetRequest(listener_handle) => {
                trace!("listener_handle: {:?}", listener_handle);

                // Respond after cm event connect request
                assert!(self.cmd_buffer.replace(req.clone()).is_none());
                Err(Error::InProgress)
                // let listener = self.state.resource().cmid_table.get(listener_handle)?;
                // let new_cmid = listener.get_request().map_err(Error::RdmaCm)?;

                // let (new_cmid_handle, handles) = self.state.resource().insert_cmid(new_cmid)?;
                // let ret_qp = prepare_returned_qp(handles);
                // let ret_cmid = returned::CmId {
                //     handle: interface::CmId(new_cmid_handle),
                //     qp: ret_qp,
                // };
                // Ok(ResponseKind::GetRequest(ret_cmid))
            }
            Request::Accept(cmid_handle, conn_param) => {
                trace!(
                    "Accept, cmid_handle: {:?}, conn_param: {:?}",
                    cmid_handle,
                    conn_param
                );
                let cmid = self.state.resource().cmid_table.get(&cmid_handle)?;
                cmid.accept(self.get_conn_param(conn_param).as_ref())
                    .map_err(Error::RdmaCm)?;

                assert!(self.cmd_buffer.replace(req.clone()).is_none());
                Err(Error::InProgress)
                // Ok(ResponseKind::Accept)
            }
            Request::Connect(cmid_handle, conn_param) => {
                trace!(
                    "Connect, cmid_handle: {:?}, conn_param: {:?}",
                    cmid_handle,
                    conn_param
                );
                let cmid = self.state.resource().cmid_table.get(cmid_handle)?;
                cmid.connect(self.get_conn_param(conn_param).as_ref())
                    .map_err(Error::RdmaCm)?;

                // Respond the user after cm event connection established
                assert!(self.cmd_buffer.replace(req.clone()).is_none());
                Err(Error::InProgress)
                // Ok(ResponseKind::Connect)
            }
            Request::CreateId(port_space) => {
                trace!("CreateId, port_space: {:?}", port_space);
                // create a new event channel for each cmid
                let channel =
                    rdmacm::EventChannel::create_event_channel().map_err(Error::RdmaCm)?;
                // set nonblocking
                channel.set_nonblocking(true).map_err(Error::RdmaCm)?;
                let channel_handle: Handle = channel.handle().into();
                self.state
                    .register_event_channel(channel_handle, &channel)?;
                let ps: rdmacm::PortSpace = (*port_space).into();
                // TODO(cjr): this is safe because event_channel will be stored in the
                // ResourceTable
                let cmid =
                    unsafe { CmId::create_id(Some(&channel), 0, ps.0) }.map_err(Error::RdmaCm)?;
                // insert event_channel
                self.state
                    .resource()
                    .event_channel_table
                    .insert(channel_handle, channel)?;
                // insert cmid after event_channel is inserted
                let new_cmid_handle = self.state.resource().insert_cmid(cmid)?;
                let ret_cmid = returned::CmId {
                    handle: interface::CmId(new_cmid_handle),
                    qp: None,
                };
                Ok(ResponseKind::CreateId(ret_cmid))
            }
            Request::BindAddr(cmid_handle, sockaddr) => {
                trace!(
                    "BindAddr: cmid_handle: {:?}, sockaddr: {:?}",
                    cmid_handle,
                    sockaddr
                );
                let cmid = self.state.resource().cmid_table.get(cmid_handle)?;
                cmid.bind_addr(&sockaddr).map_err(Error::RdmaCm)?;
                Ok(ResponseKind::BindAddr)
            }
            Request::ResolveAddr(cmid_handle, sockaddr) => {
                trace!(
                    "ResolveAddr: cmid_handle: {:?}, sockaddr: {:?}",
                    cmid_handle,
                    sockaddr
                );
                let cmid = self.state.resource().cmid_table.get(cmid_handle)?;
                cmid.resolve_addr(&sockaddr).map_err(Error::RdmaCm)?;
                assert!(self.cmd_buffer.replace(req.clone()).is_none());
                Err(Error::InProgress)
                // Ok(ResponseKind::ResolveAddr)
            }
            Request::ResolveRoute(cmid_handle, timeout_ms) => {
                trace!(
                    "ResolveRoute: cmid_handle: {:?}, timeout_ms: {:?}",
                    cmid_handle,
                    timeout_ms
                );
                let cmid = self.state.resource().cmid_table.get(cmid_handle)?;
                cmid.resolve_route(*timeout_ms).map_err(Error::RdmaCm)?;
                assert!(self.cmd_buffer.replace(req.clone()).is_none());
                Err(Error::InProgress)
                // Ok(ResponseKind::ResolveRoute)
            }
            Request::CmCreateQp(cmid_handle, pd, qp_init_attr) => {
                trace!(
                    "CmCreateQp: cmid_handle: {:?}, pd: {:?}, qp_init_attr: {:?}",
                    cmid_handle,
                    pd,
                    qp_init_attr
                );
                let cmid = self.state.resource().cmid_table.get(cmid_handle)?;

                let pd = pd.or_else(|| {
                    // use the default pd of the corresponding device
                    let sgid = cmid.sgid();
                    Some(self.state.resource().default_pd(&sgid))
                });

                let (pd, qp_init_attr) = self.get_qp_params(&pd, Some(qp_init_attr))?;
                let qp = cmid
                    .create_qp(pd.as_deref(), qp_init_attr.as_ref())
                    .map_err(Error::RdmaCm)?;
                let handles = self.state.resource().insert_qp(qp)?;
                let ret_qp = prepare_returned_qp(handles);
                Ok(ResponseKind::CmCreateQp(ret_qp))
            }
            Request::RegMr(pd, nbytes, access) => {
                trace!(
                    "RegMr, pd: {:?}, nbytes: {}, access: {:?}",
                    pd,
                    nbytes,
                    access
                );
                let pd = self.state.resource().pd_table.get(&pd.0)?;
                let mr = rdma::mr::MemoryRegion::new(&pd, *nbytes, *access)
                    .map_err(Error::MemoryRegion)?;
                let fd = mr.memfd().as_raw_fd();
                self.sock
                    .send_fd(&self.client_path, &[fd][..])
                    .map_err(Error::SendFd)?;
                let rkey = mr.rkey();
                let new_mr_handle = mr.handle().into();
                self.state.resource().mr_table.insert(new_mr_handle, mr)?;
                Ok(ResponseKind::RegMr(returned::MemoryRegion {
                    handle: interface::MemoryRegion(new_mr_handle),
                    rkey,
                }))
            }
            Request::DeregMr(mr) => {
                trace!("DeregMr, mr: {:?}", mr);
                self.state.resource().mr_table.close_resource(&mr.0)?;
                Ok(ResponseKind::DeregMr)
            }
            Request::DeallocPd(pd) => {
                trace!("DeallocPd, pd: {:?}", pd);
                self.state.resource().pd_table.close_resource(&pd.0)?;
                Ok(ResponseKind::DeallocPd)
            }
            Request::DestroyCq(cq) => {
                trace!("DestroyQp, cq: {:?}", cq);
                self.state.resource().cq_table.close_resource(&cq.0)?;
                Ok(ResponseKind::DestroyCq)
            }
            Request::DestroyQp(qp) => {
                trace!("DestroyQp, qp: {:?}", qp);
                self.state.resource().qp_table.close_resource(&qp.0)?;
                Ok(ResponseKind::DestroyQp)
            }
            Request::Disconnect(cmid) => {
                trace!("Disconnect, cmid: {:?}", cmid);
                let cmid = self.state.resource().cmid_table.get(&cmid.0)?;
                cmid.disconnect().map_err(Error::RdmaCm)?;
                assert!(self.cmd_buffer.replace(req.clone()).is_none());
                Err(Error::InProgress)
                // Ok(ResponseKind::Disconnect)
            }
            Request::DestroyId(cmid) => {
                trace!("DestroyId, cmid: {:?}", cmid);
                self.state.resource().cmid_table.close_resource(&cmid.0)?;
                Ok(ResponseKind::DestroyId)
            }

            Request::OpenPd(pd) => {
                trace!("OpenPd, pd: {:?}", pd);
                self.state.resource().pd_table.open_resource(&pd.0)?;
                Ok(ResponseKind::OpenPd)
            }
            Request::OpenCq(cq) => {
                trace!("OpenCq, cq: {:?}", cq);
                self.state.resource().cq_table.open_resource(&cq.0)?;
                Ok(ResponseKind::OpenCq)
            }
            Request::OpenQp(qp) => {
                trace!("OpenQp, qp: {:?}", qp);
                self.state.resource().qp_table.open_resource(&qp.0)?;
                Ok(ResponseKind::OpenQp)
            }
        }
    }
}
