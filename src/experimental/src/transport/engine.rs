use std::collections::HashMap;
use std::collections::VecDeque;
use std::fs;
use std::fs::File;
use std::io;
use std::mem;
use std::mem::MaybeUninit;
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::ptr;
use std::slice;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use uuid::Uuid;

use interface::returned;
use interface::Handle;
use ipc::{self, buf, cmd, dp};

use engine::{Engine, SchedulingMode, Upgradable, Version};

use rdma::ibv;
use rdma::rdmacm;
use rdma::rdmacm::CmId;

use crate::transport::{DatapathError, Error};

/// A variety of tables map a `Handle` to a kind of RNIC resource.
#[derive(Default)]
struct Resource<'ctx> {
    cmid_cnt: usize,
    pd_table: ResourceTable<ibv::ProtectionDomain<'ctx>>,
    cq_table: ResourceTable<ibv::CompletionQueue<'ctx>>,
    qp_table: ResourceTable<ibv::QueuePair<'ctx>>,
    cmid_table: ResourceTable<CmId>,
    mr_table: ResourceTable<rdmacm::MemoryRegion>,
}

#[derive(Debug)]
struct ResourceTable<R> {
    table: HashMap<Handle, R>,
}

impl<R> Default for ResourceTable<R> {
    fn default() -> Self {
        ResourceTable {
            table: HashMap::new(),
        }
    }
}

impl<R> ResourceTable<R> {
    fn insert(&mut self, h: Handle, r: R) -> Result<(), Error> {
        match self.table.insert(h, r) {
            Some(_) => Err(Error::Exists),
            None => Ok(()),
        }
    }
    fn get(&self, h: &Handle) -> Result<&R, Error> {
        self.table.get(h).ok_or(Error::NotFound)
    }
    fn get_dp(&self, h: &Handle) -> Result<&R, DatapathError> {
        self.table.get(h).ok_or(DatapathError::NotFound)
    }
}

/// This table should be shared between multiple transport engines, it must allow concurrent access
/// and modification. But for now, we implement it without considering synchronization.
#[derive(Debug)]
struct MemoryTranslationTable {
    // mr_handle -> SharedMemoryFile
    table: HashMap<Handle, SharedMemoryFile>,
}

/// A piece of memory region that is both mmap shared and registered in the NIC.
#[derive(Debug)]
struct SharedMemoryFile {
    ubuf: buf::Buffer,
    kbuf: &'static mut [u8],
    memfile: File,
    path: PathBuf,
}

impl SharedMemoryFile {
    fn new<P: AsRef<Path>>(ubuf: buf::Buffer, buffer: &mut [u8], memfile: File, path: P) -> Self {
        SharedMemoryFile {
            ubuf,
            kbuf: unsafe { slice::from_raw_parts_mut(buffer.as_mut_ptr(), buffer.len()) },
            memfile,
            path: path.as_ref().to_owned(),
        }
    }
}

impl MemoryTranslationTable {
    fn new() -> Self {
        MemoryTranslationTable {
            table: Default::default(),
        }
    }

    fn allocate(&mut self, mr_handle: Handle, smf: SharedMemoryFile) {
        self.table.insert(mr_handle, smf).ok_or(()).unwrap_err();
    }
}

impl<'ctx> Resource<'ctx> {
    fn new() -> Self {
        Default::default()
    }

    fn allocate_new_cmid_handle(&mut self) -> Handle {
        let ret = Handle(self.cmid_cnt);
        self.cmid_cnt += 1;
        ret
    }

    fn insert_cmid(
        &mut self,
        cmid: CmId,
    ) -> Result<(Handle, Option<(Handle, Handle, Handle)>), Error> {
        let cmid_handle = self.allocate_new_cmid_handle();
        if let Some(qp) = cmid.qp() {
            let send_cq = qp.send_cq();
            let recv_cq = qp.recv_cq();
            let scq_handle = send_cq.handle().into();
            let rcq_handle = recv_cq.handle().into();
            let qp_handle = qp.handle().into();
            self.cmid_table.insert(cmid_handle, cmid)?;
            self.qp_table.insert(qp_handle, qp)?;
            // safely ignore the error if the cq has already been created.
            let _ = self.cq_table.insert(scq_handle, send_cq);
            let _ = self.cq_table.insert(rcq_handle, recv_cq);
            let handles = Some((qp_handle, scq_handle, rcq_handle));
            Ok((cmid_handle, handles))
        } else {
            // passive cmid, no QP associated.
            self.cmid_table.insert(cmid_handle, cmid)?;
            Ok((cmid_handle, None))
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
    _mode: SchedulingMode,
    dp_spin_cnt: usize,
    backoff: usize,

    resource: Resource<'ctx>,
    mtt: MemoryTranslationTable,
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
    ) -> Self {
        let uuid = Uuid::new_v4();
        let sock_path = PathBuf::from(format!("/tmp/koala/koala-transport-engine-{}.sock", uuid));

        if sock_path.exists() {
            // This is impossible using uuid.
            fs::remove_file(&sock_path).expect("remove_file");
        }
        let sock = UnixDatagram::bind(&sock_path).expect("create unix domain socket failed");
        TransportEngine {
            client_path: client_path.as_ref().to_owned(),
            sock,
            cmd_rx_entries,
            cmd_tx,
            cmd_rx,
            dp_wq,
            dp_cq,
            cq_err_buffer: VecDeque::new(),
            _mode: mode,
            dp_spin_cnt: 0,
            backoff: 1,
            resource: Resource::new(),
            mtt: MemoryTranslationTable::new(), // it should be passed in from the transport module
        }
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

    fn resume(&mut self) {
        unimplemented!();
    }
}

impl<'ctx> Engine for TransportEngine<'ctx> {
    fn init(&mut self) {
        unimplemented!();
    }

    fn run(&mut self) -> bool {
        const DP_LIMIT: usize = 1 << 17;
        match self.check_dp() {
            Ok(n) => {
                if n > 0 {
                    self.backoff = DP_LIMIT.min(self.backoff * 2);
                }
            }
            Err(_) => return true,
        }

        self.dp_spin_cnt += 1;
        if self.dp_spin_cnt < self.backoff {
            return false;
        }

        self.dp_spin_cnt = 0;

        if self.cmd_rx_entries.load(Ordering::Acquire) > 0 {
            self.backoff = std::cmp::max(1, self.backoff / 2);
            if self.flush_dp() {
                return true;
            }
            if self.check_cmd() {
                return true;
            }
        } else {
            self.backoff = DP_LIMIT.min(self.backoff * 2);
        }

        false
    }

    fn shutdown(&mut self) {
        unimplemented!();
    }

    fn enqueue(&self) {
        unimplemented!();
    }

    fn check_queue_len(&self) {
        unimplemented!();
    }
}

impl<'ctx> TransportEngine<'ctx> {
    fn flush_dp(&mut self) -> bool {
        let mut processed = 0;
        let existing_work = match self.dp_wq.receiver_mut().read_count() {
            Ok(n) => n,
            Err(e) => return true,
        };

        while processed < existing_work {
            match self.check_dp() {
                Ok(n) => processed += n,
                Err(_) => return true,
            }
        }

        false
    }

    fn check_dp(&mut self) -> Result<usize, DatapathError> {
        use dp::WorkRequest;
        const BUF_LEN: usize = 32;

        // Fetch available work requests. Copy them into a buffer.
        let mut count = BUF_LEN.min(self.dp_cq.sender_mut().write_count()?);
        if count == 0 {
            return Ok(0);
        }

        let mut buffer = Vec::with_capacity(BUF_LEN);

        self.dp_wq
            .receiver_mut()
            .recv(|ptr, read_count| unsafe {
                // TODO(cjr): One optimization is to post all available send requests in one batch
                // using doorbell
                debug_assert!(count <= BUF_LEN);
                count = count.min(read_count);
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

        Ok(count)
    }

    fn check_cmd(&mut self) -> bool {
        match self.cmd_rx.try_recv() {
            // handle request
            Ok(req) => {
                self.cmd_rx_entries.fetch_sub(1, Ordering::AcqRel);
                let result = self.process_cmd(&req);
                match result {
                    Ok(res) => self.cmd_tx.send(cmd::Response(Ok(res))),
                    Err(e) => self.cmd_tx.send(cmd::Response(Err(e.into()))),
                }
                .map_or_else(|_| true, |_| false)
            }
            Err(ipc::TryRecvError::Empty) => {
                // do nothing
                false
            }
            Err(ipc::TryRecvError::IpcError(e)) => {
                if matches!(e, ipc::IpcError::Disconnected) {
                    return true;
                }
                panic!("recv error: {:?}", e);
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
        }
    }

    fn get_completion_from_error(&self, wr: &dp::WorkRequest, e: DatapathError) -> dp::Completion {
        use interface::{WcStatus, WorkCompletion};
        use rdma::ffi::ibv_wc_status;
        use std::num::NonZeroU32;

        let (cq_handle, wr_id) = self.get_dp_error_info(wr);
        dp::Completion {
            cq_handle,
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
            WorkRequest::PostRecv(cmid_handle, wr_id, user_buf, mr_handle) => {
                // trace!(
                //     "cmid_handle: {:?}, wr_id: {:?}, user_buf: {:x?}, mr_handle: {:?}",
                //     cmid_handle,
                //     wr_id,
                //     user_buf,
                //     mr_handle
                // );
                let cmid = self.resource.cmid_table.get_dp(cmid_handle)?;
                let mr = self.resource.mr_table.get_dp(mr_handle)?;
                let smf = self
                    .mtt
                    .table
                    .get_mut(mr_handle)
                    .ok_or(DatapathError::NotFound)?;

                if smf.ubuf.addr > user_buf.addr
                    || smf.ubuf.addr + smf.ubuf.len < user_buf.addr + user_buf.len
                {
                    return Err(DatapathError::OutOfRange);
                }

                let offset = (user_buf.addr - smf.ubuf.addr) as usize;
                let len = user_buf.len as usize;
                let buf = &mut smf.kbuf[offset..offset + len];
                unsafe { cmid.post_recv(*wr_id, buf, mr) }.map_err(DatapathError::RdmaCm)?;
                Ok(())
            }
            WorkRequest::PostSend(cmid_handle, wr_id, user_buf, mr_handle, send_flags) => {
                // trace!(
                //     "cmid_handle: {:?}, wr_id: {:?}, user_buf: {:x?}, mr_handle: {:?}, send_flags: {:?}",
                //     cmid_handle,
                //     wr_id,
                //     user_buf,
                //     mr_handle,
                //     send_flags,
                // );
                let cmid = self.resource.cmid_table.get_dp(cmid_handle)?;
                let mr = self.resource.mr_table.get_dp(mr_handle)?;
                let smf = self
                    .mtt
                    .table
                    .get_mut(mr_handle)
                    .ok_or(DatapathError::NotFound)?;

                if smf.ubuf.addr > user_buf.addr
                    || smf.ubuf.addr + smf.ubuf.len < user_buf.addr + user_buf.len
                {
                    return Err(DatapathError::OutOfRange);
                }

                let offset = (user_buf.addr - smf.ubuf.addr) as usize;
                let len = user_buf.len as usize;
                let buf = &smf.kbuf[offset..offset + len];
                let flags: ibv::SendFlags = (*send_flags).into();
                unsafe { cmid.post_send(*wr_id, buf, mr, flags.0) }
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
                            let mut wc = slice::from_raw_parts_mut(handle_ptr.add(1).cast(), 1);
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

    /// Process control path operations.
    fn process_cmd(&mut self, req: &cmd::Request) -> Result<cmd::ResponseKind, Error> {
        use ipc::cmd::{Request, ResponseKind};
        match req {
            Request::NewClient(..) => unreachable!(),
            &Request::Hello(number) => Ok(ResponseKind::HelloBack(number)),
            Request::GetAddrInfo(node, service, hints) => {
                trace!(
                    "node: {:?}, service: {:?}, hints: {:?}",
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
            Request::CreateEp(ai, pd_handle, qp_init_attr) => {
                trace!(
                    "ai: {:?}, pd_handle: {:?}, qp_init_attr: {:?}",
                    ai,
                    pd_handle,
                    qp_init_attr
                );

                let pd = if let Some(h) = pd_handle {
                    Some(self.resource.pd_table.get(h)?)
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

                match CmId::create_ep(&ai.clone().into(), pd, qp_init_attr.as_ref()) {
                    Ok(cmid) => {
                        let (cmid_handle, handles) = self.resource.insert_cmid(cmid)?;
                        let ret_qp = if let Some((qp_handle, scq_handle, rcq_handle)) = handles {
                            Some(returned::QueuePair {
                                handle: interface::QueuePair(qp_handle),
                                send_cq: returned::CompletionQueue {
                                    handle: interface::CompletionQueue(scq_handle),
                                },
                                recv_cq: returned::CompletionQueue {
                                    handle: interface::CompletionQueue(rcq_handle),
                                },
                            })
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
                trace!("cmid_handle: {:?}, backlog: {}", cmid_handle, backlog);

                let listener = self.resource.cmid_table.get(cmid_handle)?;

                listener.listen(*backlog).map_err(Error::RdmaCm)?;

                Ok(ResponseKind::Listen)
            }
            Request::GetRequest(cmid_handle) => {
                trace!("cmid_handle: {:?}", cmid_handle);

                let listener = self.resource.cmid_table.get(cmid_handle)?;
                let new_cmid = listener.get_request().map_err(Error::RdmaCm)?;

                let (new_cmid_handle, handles) = self.resource.insert_cmid(new_cmid)?;
                let ret_qp = if let Some((qp_handle, scq_handle, rcq_handle)) = handles {
                    Some(returned::QueuePair {
                        handle: interface::QueuePair(qp_handle),
                        send_cq: returned::CompletionQueue {
                            handle: interface::CompletionQueue(scq_handle),
                        },
                        recv_cq: returned::CompletionQueue {
                            handle: interface::CompletionQueue(rcq_handle),
                        },
                    })
                } else {
                    None
                };
                let ret_cmid = returned::CmId {
                    handle: interface::CmId(new_cmid_handle),
                    qp: ret_qp,
                };
                Ok(ResponseKind::GetRequest(ret_cmid))
            }
            Request::Accept(cmid_handle, conn_param) => {
                trace!(
                    "cmid_handle: {:?}, conn_param: {:?}",
                    cmid_handle,
                    conn_param
                );
                warn!("TODO: conn_param is ignored for now");
                let listener = self.resource.cmid_table.get(cmid_handle)?;
                listener.accept().map_err(Error::RdmaCm)?;

                Ok(ResponseKind::Accept)
            }
            Request::Connect(cmid_handle, conn_param) => {
                trace!(
                    "cmid_handle: {:?}, conn_param: {:?}",
                    cmid_handle,
                    conn_param
                );
                warn!("TODO: conn_param is ignored for now");
                let cmid = self.resource.cmid_table.get(cmid_handle)?;
                cmid.connect().map_err(Error::RdmaCm)?;

                Ok(ResponseKind::Connect)
            }
            Request::RegMsgs(cmid_handle, user_buf) => {
                trace!("cmid_handle: {:?}, user_buf: {:x?}", cmid_handle, user_buf);
                let cmid = self.resource.cmid_table.get(cmid_handle)?;
                // 1. create/open shared memory file
                use nix::fcntl::OFlag;
                use nix::sys::mman::{mmap, munmap, shm_open, shm_unlink, MapFlags, ProtFlags};
                use nix::sys::stat::Mode;
                // just randomly pick an string and use that for now
                let shm_path = PathBuf::from(format!("koala-{}", Uuid::new_v4())); // no /dev/shm prefix is needed
                let fd = shm_open(
                    &shm_path,
                    OFlag::O_CREAT | OFlag::O_RDWR | OFlag::O_EXCL,
                    Mode::S_IRUSR | Mode::S_IWUSR,
                )
                .map_err(Error::ShmOpen)?;
                // 2. mmap the user's vaddr into koala's address space through
                // __linear__ mapping (this is necessary) (actually, maybe not)
                let uaddr = user_buf.addr as usize;
                let ulen = user_buf.len as usize;
                let page_size = 4096;
                let aligned_end = (uaddr + ulen + page_size - 1) / page_size * page_size;
                let aligned_begin = uaddr - uaddr % page_size;
                let aligned_len = aligned_end - aligned_begin;
                // ftruncate the file
                let memfile = unsafe { File::from_raw_fd(fd) };
                memfile.set_len(aligned_len as _).map_err(Error::Truncate)?;
                let kaddr = unsafe {
                    mmap(
                        ptr::null_mut(),
                        aligned_len,
                        ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                        MapFlags::MAP_SHARED | MapFlags::MAP_NORESERVE,
                        fd,
                        0,
                    )
                    .map_err(Error::Mmap)?
                };
                // 3. send fd back
                ipc::send_fd(&self.sock, &self.client_path, &[fd][..]).map_err(Error::SendFd)?;
                // 4. register kaddr with ibv_reg_mr
                let buf = unsafe { slice::from_raw_parts_mut(kaddr as *mut u8, aligned_len) };
                let mr = cmid.reg_msgs(&buf).map_err(Error::RdmaCm)?;
                // 5. allocate mr handle
                let new_mr_handle = mr.handle().into();
                self.resource.mr_table.insert(new_mr_handle, mr)?;
                // 6. allocate entry in MTT (mr -> SharedMemoryFile)
                let kbuf = &mut buf[uaddr - aligned_begin..uaddr - aligned_begin + ulen];
                let smf = SharedMemoryFile::new(*user_buf, kbuf, memfile, &shm_path);
                self.mtt.allocate(new_mr_handle, smf);
                // 7. send mr handle back
                Ok(ResponseKind::RegMsgs(new_mr_handle))
            }
        }
    }
}
