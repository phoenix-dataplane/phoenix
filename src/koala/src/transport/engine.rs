use std::collections::HashMap;
use std::collections::VecDeque;
use std::fs;
use std::io;
use std::marker::PhantomPinned;
use std::mem;
use std::mem::ManuallyDrop;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::slice;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use lazy_static::lazy_static;
use memoffset;
use uuid::Uuid;

use interface::{returned, Handle};
use ipc::{self, cmd, dp};

use engine::{Engine, EngineStatus, SchedulingMode, Upgradable, Version};

use rdma::ibv;
use rdma::rdmacm;
use rdma::rdmacm::CmId;

use super::resource::ResourceTable;
use super::{DatapathError, Error};

lazy_static! {
    static ref DEFAULT_CTXS: Vec<(Pin<Box<PinnedContext>>, Vec<ibv::Gid>)> =
        open_default_verbs().expect("Open default RDMA context failed.");
}

/// Open default verbs contexts
fn open_default_verbs() -> io::Result<Vec<(Pin<Box<PinnedContext>>, Vec<ibv::Gid>)>> {
    let mut default_ctxs = Vec::new();
    let ctx_list = rdmacm::get_devices()?;
    for ctx in ctx_list.into_iter() {
        let result: io::Result<_> = (|| {
            let max_index = ctx.port_attr()?.gid_tbl_len as usize;
            let gid_table: io::Result<_> = (0..max_index).map(|index| ctx.gid(index)).collect();
            Ok((ctx, gid_table?))
        })();
        match result {
            Ok((ctx, gid_table)) => {
                default_ctxs.push((Box::pin(PinnedContext::new(ctx)), gid_table));
            }
            Err(e) => {
                warn!("Skip device due to: {}", e);
                continue;
            }
        }
    }

    if default_ctxs.is_empty() {
        warn!("No active RDMA device found.");
    }
    Ok(default_ctxs)
}

/// A variety of tables where each maps a `Handle` to a kind of RNIC resource.
struct Resource<'ctx> {
    cmid_cnt: u32,
    default_pds: HashMap<ibv::Gid, interface::ProtectionDomain>,
    // NOTE(cjr): Do NOT change the order of the following fields. A wrong drop order may cause
    // failures in the underlying library.
    cmid_table: ResourceTable<CmId>,
    event_channel_table: ResourceTable<rdmacm::EventChannel>,
    qp_table: ResourceTable<ibv::QueuePair<'ctx>>,
    mr_table: ResourceTable<rdma::mr::MemoryRegion>,
    cq_table: ResourceTable<ibv::CompletionQueue<'ctx>>,
    pd_table: ResourceTable<ibv::ProtectionDomain<'ctx>>,
}

impl<'ctx> Resource<'ctx> {
    fn new() -> io::Result<Self> {
        let mut default_pds = HashMap::new();
        let mut pd_table = ResourceTable::default();
        for (ctx, gid_table) in DEFAULT_CTXS.iter() {
            let pd = match ctx.verbs.alloc_pd() {
                Ok(pd) => pd,
                Err(_) => continue,
            };
            let pd_handle = pd.handle().into();
            pd_table.insert(pd_handle, pd).unwrap();
            for gid in gid_table {
                default_pds.insert(*gid, interface::ProtectionDomain(pd_handle));
            }
        }
        Ok(Resource {
            cmid_cnt: 0,
            default_pds,
            cmid_table: ResourceTable::default(),
            event_channel_table: ResourceTable::default(),
            qp_table: ResourceTable::default(),
            mr_table: ResourceTable::default(),
            cq_table: ResourceTable::default(),
            pd_table,
        })
    }

    fn allocate_new_cmid_handle(&mut self) -> Handle {
        let ret = Handle(self.cmid_cnt);
        self.cmid_cnt += 1;
        ret
    }

    fn insert_qp(
        &mut self,
        qp: ibv::QueuePair<'ctx>,
    ) -> Result<(Handle, Handle, Handle, Handle), Error> {
        let pd = qp.pd();
        let send_cq = qp.send_cq();
        let recv_cq = qp.recv_cq();
        let pd_handle = pd.handle().into();
        let scq_handle = send_cq.handle().into();
        let rcq_handle = recv_cq.handle().into();
        let qp_handle = qp.handle().into();
        // qp, cqs, and other associated resources are supposed to be open by the user
        // explicited, the error should be safely ignored if the resource has already been
        // created.
        self.qp_table.occupy_or_create_resource(qp_handle, qp);
        self.pd_table.occupy_or_create_resource(pd_handle, pd);
        self.cq_table.occupy_or_create_resource(scq_handle, send_cq);
        self.cq_table.occupy_or_create_resource(rcq_handle, recv_cq);
        Ok((qp_handle, pd_handle, scq_handle, rcq_handle))
    }

    fn insert_cmid(
        &mut self,
        cmid: CmId,
    ) -> Result<(Handle, Option<(Handle, Handle, Handle, Handle)>), Error> {
        let cmid_handle = self.allocate_new_cmid_handle();
        if let Some(qp) = cmid.qp() {
            self.cmid_table.insert(cmid_handle, cmid)?;
            let handles = Some(self.insert_qp(qp)?);
            Ok((cmid_handle, handles))
        } else {
            // listener cmid, no QP associated.
            self.cmid_table.insert(cmid_handle, cmid)?;
            Ok((cmid_handle, None))
        }
    }
}

struct PinnedContext {
    verbs: ManuallyDrop<ibv::Context>,
    _pin: PhantomPinned,
}

impl PinnedContext {
    fn new(ctx: ManuallyDrop<ibv::Context>) -> Self {
        PinnedContext {
            verbs: ctx,
            _pin: PhantomPinned,
        }
    }
}

pub struct TransportEngine<'ctx> {
    /// This is the path of the domain socket which is client side is listening on.
    /// The mainly purpose of keeping is to send file descriptors to the client.
    client_path: PathBuf,
    sock: UnixDatagram,
    cmd_rx_entries: ipc::ShmObject<AtomicUsize>,
    cmd_tx: ipc::IpcSender<cmd::Response>,
    cmd_rx: ipc::IpcReceiver<cmd::Request>,
    dp_wq: ipc::ShmReceiver<dp::WorkRequestSlot>,
    dp_cq: ipc::ShmSender<dp::CompletionSlot>,
    cq_err_buffer: VecDeque<dp::Completion>,

    dp_spin_cnt: usize,
    backoff: usize,
    _mode: SchedulingMode,

    resource: Resource<'ctx>,

    poll: mio::Poll,
    // bufferred control path request
    cmd_buffer: Option<cmd::Request>,
    // otherwise, the
    last_cmd_ts: Instant,
}

impl<'ctx> TransportEngine<'ctx> {
    pub fn new<P: AsRef<Path>>(
        client_path: P,
        cmd_rx_entries: ipc::ShmObject<AtomicUsize>,
        cmd_tx: ipc::IpcSender<cmd::Response>,
        cmd_rx: ipc::IpcReceiver<cmd::Request>,
        dp_wq: ipc::ShmReceiver<dp::WorkRequestSlot>,
        dp_cq: ipc::ShmSender<dp::CompletionSlot>,
        mode: SchedulingMode,
    ) -> io::Result<Self> {
        let uuid = Uuid::new_v4();
        let sock_path = PathBuf::from(format!("/tmp/koala/koala-transport-engine-{}.sock", uuid));

        if sock_path.exists() {
            // This is impossible using uuid.
            fs::remove_file(&sock_path)?;
        }
        let sock = UnixDatagram::bind(&sock_path)?;

        Ok(TransportEngine {
            client_path: client_path.as_ref().to_owned(),
            sock,
            cmd_rx_entries,
            cmd_tx,
            cmd_rx,
            dp_wq,
            dp_cq,
            cq_err_buffer: VecDeque::new(),
            dp_spin_cnt: 0,
            backoff: 1,
            _mode: mode,
            resource: Resource::new()?,
            poll: mio::Poll::new()?,
            cmd_buffer: None,
            last_cmd_ts: Instant::now(),
        })
    }
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

fn prepare_returned_qp(
    handles: Option<(Handle, Handle, Handle, Handle)>,
) -> Option<returned::QueuePair> {
    if let Some((qp_handle, pd_handle, scq_handle, rcq_handle)) = handles {
        Some(returned::QueuePair {
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
        })
    } else {
        None
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
                    // koala system rather than the specified by the user. Image that the user is
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
        match self.poll_cm_event_once() {
            Ok(cm_event) => {
                // the event must be consumed
                match self.process_cm_event(cm_event) {
                    Ok(res) => self.cmd_tx.send(cmd::Response(Ok(res)))?,
                    Err(e) => self.cmd_tx.send(cmd::Response(Err(e.into())))?,
                }
                Ok(Progress(1))
            }
            Err(Error::NoCmEvent) => {
                // poll again next time
                Ok(Progress(0))
            }
            Err(e @ (Error::RdmaCm(_) | Error::Mio(_))) => {
                self.cmd_tx.send(cmd::Response(Err(e.into())))?;
                Ok(Progress(1))
            }
            Err(e) => Err(e),
        }
    }

    /// Return the cq_handle and wr_id for the work request
    fn get_dp_error_info(&self, wr: &dp::WorkRequest) -> (interface::CompletionQueue, u64) {
        use dp::WorkRequest;
        match wr {
            WorkRequest::PostSend(cmid_handle, wr_id, ..) => {
                // if the cq_handle does not exists at all, set it to
                // Handle::INVALID.
                if let Ok(cmid) = self.resource.cmid_table.get_dp(cmid_handle) {
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
                if let Ok(cmid) = self.resource.cmid_table.get_dp(cmid_handle) {
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
                if let Ok(cmid) = self.resource.cmid_table.get_dp(cmid_handle) {
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
                if let Ok(cmid) = self.resource.cmid_table.get_dp(cmid_handle) {
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
                let cmid = self.resource.cmid_table.get_dp(cmid_handle)?;
                let mr = self.resource.mr_table.get_mut_dp(mr_handle)?;

                let rdma_mr = mr.into();
                let buf = &mut mr[range.offset as usize..(range.offset + range.len) as usize];

                unsafe { cmid.post_recv(*wr_id, buf, &rdma_mr) }.map_err(DatapathError::RdmaCm)?;
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
                let cmid = self.resource.cmid_table.get_dp(cmid_handle)?;
                let mr = self.resource.mr_table.get_dp(mr_handle)?;

                let rdma_mr = mr.into();
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
                let cmid = self.resource.cmid_table.get_dp(cmid_handle)?;
                let mr = self.resource.mr_table.get_dp(mr_handle)?;

                let rdma_mr = mr.into();
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
                let cmid = self.resource.cmid_table.get_dp(cmid_handle)?;
                let mr = self.resource.mr_table.get_mut_dp(mr_handle)?;

                let rdma_mr = mr.into();
                let buf = &mut mr[range.offset as usize..(range.offset + range.len) as usize];
                let remote_addr = rkey.addr + remote_offset;

                let flags: ibv::SendFlags = (*send_flags).into();
                unsafe { cmid.post_read(*wr_id, buf, &rdma_mr, flags.0, remote_addr, rkey.rkey) }
                    .map_err(DatapathError::RdmaCm)?;

                Ok(())
            }
            WorkRequest::PollCq(cq_handle) => {
                // trace!("cq_handle: {:?}", cq_handle);
                self.try_flush_cq_err_buffer()?;
                let cq = self.resource.cq_table.get_dp(&cq_handle.0)?;

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

    fn get_qp_params<'a>(
        &'a self,
        pd_handle: &Option<interface::ProtectionDomain>,
        qp_init_attr: Option<&interface::QpInitAttr>,
    ) -> Result<
        (
            Option<&'a ibv::ProtectionDomain<'a>>,
            Option<rdma::ffi::ibv_qp_init_attr>,
        ),
        Error,
    > {
        let pd = if let Some(h) = pd_handle {
            Some(self.resource.pd_table.get(&h.0)?)
        } else {
            None
        };
        let qp_init_attr = if let Some(a) = qp_init_attr {
            let send_cq = if let Some(ref h) = a.send_cq {
                Some(self.resource.cq_table.get(&h.0)?)
            } else {
                None
            };
            let recv_cq = if let Some(ref h) = a.recv_cq {
                Some(self.resource.cq_table.get(&h.0)?)
            } else {
                None
            };
            let attr = ibv::QpInitAttr {
                qp_context: 0,
                send_cq: send_cq,
                recv_cq: recv_cq,
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

    fn poll_cm_event_once(&mut self) -> Result<rdmacm::CmEvent, Error> {
        let mut events = mio::Events::with_capacity(1);
        self.poll
            .poll(&mut events, Some(Duration::from_millis(1)))
            .map_err(Error::Mio)?;
        for io_event in &events {
            let handle = Handle(io_event.token().0 as u32);
            let event_channel = self.resource.event_channel_table.get(&handle)?;
            // read one event
            let cm_event = event_channel.get_cm_event().map_err(Error::RdmaCm)?;
            // reregister everytime to simulate level-trigger
            self.poll
                .registry()
                .reregister(
                    &mut mio::unix::SourceFd(&event_channel.as_raw_fd()),
                    io_event.token(),
                    mio::Interest::READABLE,
                )
                .map_err(Error::Mio)?;
            return Ok(cm_event);
        }
        Err(Error::NoCmEvent)
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
                    // let listener = self.resource.cmid_table.get(listener_handle)?;
                    // assert_eq!(
                    //     listener_handle,
                    //     &Handle::from(event.listen_id().unwrap().handle())
                    // );
                    let new_cmid = event.id();

                    let (new_cmid_handle, handles) = self.resource.insert_cmid(new_cmid)?;
                    let ret_qp = prepare_returned_qp(handles);
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
                match CmId::create_ep(&ai.clone().into(), pd, qp_init_attr.as_ref()) {
                    Ok(cmid) => {
                        let (cmid_handle, handles) = self.resource.insert_cmid(cmid)?;
                        let ret_qp = prepare_returned_qp(handles);
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
                let listener = self.resource.cmid_table.get(cmid_handle)?;
                listener.listen(*backlog).map_err(Error::RdmaCm)?;
                Ok(ResponseKind::Listen)
            }
            Request::GetRequest(listener_handle) => {
                trace!("listener_handle: {:?}", listener_handle);

                // Respond after cm event connect request
                assert!(self.cmd_buffer.replace(req.clone()).is_none());
                Err(Error::InProgress)
                // let listener = self.resource.cmid_table.get(listener_handle)?;
                // let new_cmid = listener.get_request().map_err(Error::RdmaCm)?;

                // let (new_cmid_handle, handles) = self.resource.insert_cmid(new_cmid)?;
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
                let cmid = self.resource.cmid_table.get(&cmid_handle)?;
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
                let cmid = self.resource.cmid_table.get(cmid_handle)?;
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
                self.poll
                    .registry()
                    .register(
                        &mut mio::unix::SourceFd(&channel.as_raw_fd()),
                        mio::Token(channel_handle.0 as usize),
                        mio::Interest::READABLE,
                    )
                    .map_err(Error::Mio)?;
                let ps: rdmacm::PortSpace = (*port_space).into();
                let cmid = CmId::create_id(Some(&channel), 0, ps.0).map_err(Error::RdmaCm)?;
                let (new_cmid_handle, handles) = self.resource.insert_cmid(cmid)?;
                // insert event_channel
                self.resource
                    .event_channel_table
                    .insert(channel_handle, channel)?;
                let ret_qp = prepare_returned_qp(handles);
                let ret_cmid = returned::CmId {
                    handle: interface::CmId(new_cmid_handle),
                    qp: ret_qp,
                };
                Ok(ResponseKind::CreateId(ret_cmid))
            }
            Request::BindAddr(cmid_handle, sockaddr) => {
                trace!(
                    "BindAddr: cmid_handle: {:?}, sockaddr: {:?}",
                    cmid_handle,
                    sockaddr
                );
                let cmid = self.resource.cmid_table.get(cmid_handle)?;
                cmid.bind_addr(&sockaddr).map_err(Error::RdmaCm)?;
                Ok(ResponseKind::BindAddr)
            }
            Request::ResolveAddr(cmid_handle, sockaddr) => {
                trace!(
                    "ResolveAddr: cmid_handle: {:?}, sockaddr: {:?}",
                    cmid_handle,
                    sockaddr
                );
                let cmid = self.resource.cmid_table.get(cmid_handle)?;
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
                let cmid = self.resource.cmid_table.get(cmid_handle)?;
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
                let cmid = self.resource.cmid_table.get(cmid_handle)?;

                let pd = pd.or_else(|| {
                    // use the default pd of the corresponding device
                    let sgid = cmid.sgid();
                    Some(self.resource.default_pds[&sgid])
                });

                let (pd, qp_init_attr) = self.get_qp_params(&pd, Some(qp_init_attr))?;
                cmid.create_qp(pd, qp_init_attr.as_ref())
                    .map_err(Error::RdmaCm)?;
                let qp = cmid.qp().unwrap();
                let handles = self.resource.insert_qp(qp)?;
                let ret_qp = prepare_returned_qp(Some(handles)).unwrap();
                Ok(ResponseKind::CmCreateQp(ret_qp))
            }
            Request::RegMr(pd, nbytes, access) => {
                trace!(
                    "RegMr, pd: {:?}, nbytes: {}, access: {:?}",
                    pd,
                    nbytes,
                    access
                );
                let pd = self.resource.pd_table.get(&pd.0)?;
                let mr = rdma::mr::MemoryRegion::new(pd, *nbytes, *access)
                    .map_err(Error::MemoryRegion)?;
                let fd = mr.memfd().as_raw_fd();
                ipc::send_fd(&self.sock, &self.client_path, &[fd][..]).map_err(Error::SendFd)?;
                let rkey = mr.rkey();
                let new_mr_handle = mr.handle().into();
                self.resource.mr_table.insert(new_mr_handle, mr)?;
                Ok(ResponseKind::RegMr(returned::MemoryRegion {
                    handle: interface::MemoryRegion(new_mr_handle),
                    rkey,
                }))
            }
            Request::DeregMr(mr) => {
                trace!("DeregMr, mr: {:?}", mr);
                self.resource.mr_table.close_resource(&mr.0)?;
                Ok(ResponseKind::DeregMr)
            }
            Request::DeallocPd(pd) => {
                trace!("DeallocPd, pd: {:?}", pd);
                self.resource.pd_table.close_resource(&pd.0)?;
                Ok(ResponseKind::DeallocPd)
            }
            Request::DestroyCq(cq) => {
                trace!("DestroyQp, cq: {:?}", cq);
                self.resource.cq_table.close_resource(&cq.0)?;
                Ok(ResponseKind::DestroyCq)
            }
            Request::DestroyQp(qp) => {
                trace!("DestroyQp, qp: {:?}", qp);
                self.resource.qp_table.close_resource(&qp.0)?;
                Ok(ResponseKind::DestroyQp)
            }
            Request::Disconnect(cmid) => {
                trace!("Disconnect, cmid: {:?}", cmid);
                let cmid = self.resource.cmid_table.get(&cmid.0)?;
                cmid.disconnect().map_err(Error::RdmaCm)?;
                assert!(self.cmd_buffer.replace(req.clone()).is_none());
                Err(Error::InProgress)
                // Ok(ResponseKind::Disconnect)
            }
            Request::DestroyId(cmid) => {
                trace!("DestroyId, cmid: {:?}", cmid);
                self.resource.cmid_table.close_resource(&cmid.0)?;
                Ok(ResponseKind::DestroyId)
            }

            Request::OpenPd(pd) => {
                trace!("OpenPd, pd: {:?}", pd);
                self.resource.pd_table.open_resource(&pd.0)?;
                Ok(ResponseKind::OpenPd)
            }
            Request::OpenCq(cq) => {
                trace!("OpenCq, cq: {:?}", cq);
                self.resource.cq_table.open_resource(&cq.0)?;
                Ok(ResponseKind::OpenCq)
            }
            Request::OpenQp(qp) => {
                trace!("OpenQp, qp: {:?}", qp);
                self.resource.qp_table.open_resource(&qp.0)?;
                Ok(ResponseKind::OpenQp)
            }
        }
    }
}
