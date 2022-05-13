use std::collections::VecDeque;
use std::future::Future;
use std::mem;
use std::os::unix::prelude::{AsRawFd, RawFd};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use ipc::shmalloc::ShmPtr;

use ipc::mrpc;

use interface::engine::SchedulingMode;
use interface::rpc::{MessageTemplateErased, RpcMsgType};
use interface::{AsHandle, Handle};

use super::state::{ConnectionContext, ReqContext, State, WrContext};
use super::ulib;
use super::{ControlPathError, DatapathError};
use crate::engine::{future, Engine, EngineLocalStorage, EngineResult, Indicator, Vertex};
use crate::mrpc::marshal::{MessageTemplate, RpcMessage, SgList, ShmBuf, Unmarshal};
use crate::node::Node;
use crate::salloc::state::State as SallocState;
use crate::transport::rdma::ops;

pub struct TlStorage {
    pub(crate) ops: ops::Ops,
    pub(crate) state: State,
}

/// WARNING(cjr): This this not true! I unafely mark Sync for TlStorage to cheat the compiler. I
/// have to do this because runtime.running_engines[i].els() exposes a &EngineLocalStorage, and
/// runtimes are shared between two threads, so EngineLocalStorage must be Sync.
unsafe impl Sync for TlStorage {}

unsafe impl EngineLocalStorage for TlStorage {
    #[inline]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct RpcAdapterEngine {
    pub(crate) tls: Box<TlStorage>,
    pub(crate) salloc: SallocState,

    // shared completion queue model
    pub(crate) cq: Option<ulib::uverbs::CompletionQueue>,
    pub(crate) recent_listener_handle: Option<interface::Handle>,
    pub(crate) local_buffer: VecDeque<ShmPtr<dyn RpcMessage>>,

    pub(crate) node: Node,
    pub(crate) cmd_rx: tokio::sync::mpsc::UnboundedReceiver<mrpc::cmd::Command>,
    pub(crate) cmd_tx: tokio::sync::mpsc::UnboundedSender<mrpc::cmd::Completion>,

    pub(crate) _mode: SchedulingMode,
    pub(crate) indicator: Option<Indicator>,
}

crate::unimplemented_ungradable!(RpcAdapterEngine);
crate::impl_vertex_for_engine!(RpcAdapterEngine, node);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for RpcAdapterEngine {
    type Future = impl Future<Output = EngineResult>;

    fn description(&self) -> String {
        format!(
            "RcpAdapterEngine, user pid: {:?}",
            self.tls.state.shared.pid
        )
    }

    fn set_tracker(&mut self, indicator: Indicator) {
        self.indicator = Some(indicator);
    }

    fn entry(mut self) -> Self::Future {
        Box::pin(async move { self.mainloop().await })
    }

    #[inline]
    unsafe fn els(&self) -> Option<&'static dyn EngineLocalStorage> {
        let tls = self.tls.as_ref() as *const TlStorage;
        Some(&*tls)
    }
}

impl RpcAdapterEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut work = 0;

            // check input queue
            if let Progress(n) = self.check_input_queue()? {
                work += n;
            }

            // check service
            if let Progress(n) = self.check_transport_service()? {
                work += n;
            }

            // check input command queue
            if let Progress(n) = self.check_input_cmd_queue().await? {
                work += n;
            }

            // TODO(cjr): check incoming connect request
            // the CmIdListener::get_request() is currently synchronous.
            // need to make it asynchronous and low cost to check.
            // TODO(cjr): remove this to another engine and runtime
            self.check_incoming_connection().await?;

            self.indicator.as_ref().unwrap().set_nwork(work);
            future::yield_now().await;
        }
    }
}

impl RpcAdapterEngine {
    fn get_or_init_cq(&mut self) -> &ulib::uverbs::CompletionQueue {
        // this function is not supposed to be called concurrently.
        if self.cq.is_none() {
            // TODO(cjr): we currently by default use the first ibv_context.
            let ctx_list = ulib::uverbs::get_default_verbs_contexts(&self.tls.ops).unwrap();
            let ctx = &ctx_list[0];
            self.cq = Some(ctx.create_cq(1024, 0).unwrap());
        }
        self.cq.as_ref().unwrap()
    }

    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use tokio::sync::mpsc::error::TryRecvError;
        use ulib::uverbs::SendFlags;
        while let Some(msg) = self.local_buffer.pop_front() {
            // get cmid from conn_id
            let msg_ref = unsafe { msg.as_ref() };
            let cmid_handle = msg_ref.conn_id();
            let conn_ctx = self.tls.state.resource().cmid_table.get(&cmid_handle)?;
            if conn_ctx.credit.load(Ordering::Acquire) <= 5 {
                // some random number for now TODO(cjr): update this
                self.local_buffer.push_front(msg);
                break;
            }
            let cmid = &conn_ctx.cmid;
            // Sender marshals the data (gets an SgList)
            let sglist = msg_ref.marshal();
            // Sender posts send requests from the SgList
            log::debug!("check_input_queue, sglist: {:0x?}", sglist);
            if msg_ref.is_request() {
                conn_ctx.credit.fetch_sub(sglist.0.len(), Ordering::AcqRel);
                conn_ctx.outstanding_req.lock().push_back(ReqContext {
                    call_id: msg_ref.call_id(),
                    sg_len: sglist.0.len(),
                });
            }
            // TODO(cjr): credit handle logic for response
            for (i, &sge) in sglist.0.iter().enumerate() {
                // query mr from each sge
                let mr = self.tls.state.resource().query_mr(sge)?;
                let off = sge.ptr - mr.as_ptr() as usize;
                if i + 1 < sglist.0.len() {
                    // post send
                    unsafe {
                        cmid.post_send(&mr, off..off + sge.len, 0, SendFlags::SIGNALED)?;
                    }
                } else {
                    // post send with imm
                    unsafe {
                        cmid.post_send_with_imm(
                            &mr,
                            off..off + sge.len,
                            0,
                            SendFlags::SIGNALED,
                            0,
                        )?;
                    }
                }
            }
            // Sender posts an extra SendWithImm
            return Ok(Progress(1));
        }

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                self.local_buffer.push_back(msg);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }
        Ok(Progress(0))
    }

    fn unmarshal_and_deliver_up(
        &mut self,
        sgl: SgList,
        conn_ctx: Arc<ConnectionContext>,
    ) -> Result<Status, DatapathError> {
        use crate::mrpc::codegen;
        log::debug!("unmarshal_and_deliver_up, sgl: {:0x?}", sgl);
        let mut erased = unsafe { MessageTemplateErased::unmarshal(sgl.clone(), &self.salloc.shared) }.unwrap();
        let meta = &mut unsafe { erased.as_mut() }.meta;
        meta.conn_id = conn_ctx.cmid.as_handle();

        // replenish the credits
        if meta.msg_type == RpcMsgType::Response {
            let call_id = meta.call_id;
            let mut outstanding_req = conn_ctx.outstanding_req.lock();
            let req_ctx = outstanding_req.pop_front().unwrap();
            assert_eq!(call_id, req_ctx.call_id);
            conn_ctx.credit.fetch_add(req_ctx.sg_len, Ordering::AcqRel);
            drop(outstanding_req);
        }

        let dyn_msg = match meta.msg_type {
            RpcMsgType::Request => {
                match meta.func_id {
                    0 => {
                        let mut msg = unsafe {
                            MessageTemplate::<codegen::HelloRequest>::unmarshal(sgl, &self.salloc.shared).unwrap()
                        };
                        // Safety: this is fine here because msg is already a unique
                        // pointer
                        let dyn_msg = 
                            unsafe { ShmPtr::new(msg.as_mut() as *mut dyn RpcMessage, msg.get_remote_addr()).unwrap() };
                        dyn_msg
                    }
                    _ => panic!("unknown func_id: {}, meta: {:?}", meta.func_id, meta),
                }
            }
            RpcMsgType::Response => {
                match meta.func_id {
                    0 => {
                        let mut msg = unsafe {
                            MessageTemplate::<codegen::HelloReply>::unmarshal(sgl, &self.salloc.shared).unwrap()
                        };
                        // Safety: this is fine here because msg is already a unique
                        // pointer
                        let dyn_msg = 
                            unsafe { ShmPtr::new(msg.as_mut() as *mut dyn RpcMessage, msg.get_remote_addr()).unwrap() };
                        dyn_msg
                    }
                    _ => panic!("unknown func_id: {}, meta: {:?}", meta.func_id, meta),
                }
            }
        };
        self.rx_outputs()[0].send(dyn_msg).unwrap();
        Ok(Progress(1))
    }

    fn check_transport_service(&mut self) -> Result<Status, DatapathError> {
        // check completion, and replenish some recv requests
        use interface::{WcFlags, WcOpcode, WcStatus};
        let mut comps = Vec::with_capacity(32);
        let cq = self.get_or_init_cq();
        cq.poll(&mut comps)?;
        for wc in comps {
            match wc.status {
                WcStatus::Success => {
                    match wc.opcode {
                        WcOpcode::Send => {
                            // send completed, do nothing
                        }
                        WcOpcode::Recv => {
                            let wr_ctx = self.tls.state.resource().wr_contexts.get(&wc.wr_id)?;
                            let cmid_handle = wr_ctx.conn_id;
                            let conn_ctx =
                                self.tls.state.resource().cmid_table.get(&cmid_handle)?;
                            // received a segment of RPC message
                            let sge = ShmBuf {
                                ptr: wr_ctx.mr_addr,
                                len: wc.byte_len as _, // note this byte_len is only valid for
                                                       // recv request
                            };
                            conn_ctx.receiving_sgl.lock().0.push(sge);
                            if wc.wc_flags.contains(WcFlags::WITH_IMM) {
                                // received an entire RPC message
                                use std::ops::DerefMut;
                                let sgl = mem::take(conn_ctx.receiving_sgl.lock().deref_mut());
                                self.unmarshal_and_deliver_up(sgl, Arc::clone(&conn_ctx))?;
                            }
                            // XXX(cjr): only when the upper layer app finish using the data
                            // we can repost the receives
                            let cmid = &conn_ctx.cmid;
                            let recv_mr = self
                                .tls
                                .state
                                .resource()
                                .recv_mr_table
                                .get(&Handle(wc.wr_id as u32))?;
                            unsafe {
                                // TODO(cjr): check the correctness of this unsafe operation
                                let recv_mr: &mut ulib::uverbs::MemoryRegion<u8> =
                                    &mut *(recv_mr.as_ref() as *const _ as *mut _);
                                cmid.post_recv(recv_mr, .., wc.wr_id)?;
                            }
                        }
                        WcOpcode::Invalid => panic!("invalid wc: {:?}", wc),
                        _ => panic!("Unhandled wc opcode: {:?}", wc),
                    }
                }
                WcStatus::Error(_) => {
                    eprintln!("wc failed: {:?}", wc);
                }
            }
        }
        // COMMENT(cjr): Progress(0) here is okay for now because we haven't use the progress as
        // any indicator.
        Ok(Status::Progress(0))
    }

    async fn check_incoming_connection(&mut self) -> Result<Status, ControlPathError> {
        // TODO(cjr): should check for each connection.......... shit!
        if let Some(recent) = self.recent_listener_handle.as_ref() {
            let listener = self.tls.state.resource().listener_table.get(recent)?;
            if let Some(mut builder) = listener.try_get_request()? {
                // establish connection
                let cq = self.get_or_init_cq();
                let mut pre_id = builder
                    .set_send_cq(cq)
                    .set_recv_cq(cq)
                    .set_max_send_wr(128)
                    .set_max_recv_wr(128)
                    .build()?;
                // prepare and post receive buffers
                let mut recv_mrs = Vec::with_capacity(128);
                let (returned_mrs, fds) = self.prepare_recv_buffers(&mut pre_id, &mut recv_mrs)?;
                // accept
                let id = pre_id.accept(None).await?;
                let handle = id.as_handle();
                // insert resources after connection establishment
                self.tls.state.resource().insert_cmid(id, 128)?;
                for recv_mr in recv_mrs {
                    self.tls
                        .state
                        .resource()
                        .recv_mr_table
                        .insert(recv_mr.as_handle(), recv_mr)?;
                }
                // pass these resources back to the user
                let comp = mrpc::cmd::Completion(Ok(
                    mrpc::cmd::CompletionKind::NewConnectionInternal(handle, returned_mrs, fds),
                ));
                self.cmd_tx.send(comp)?;
            }
            Ok(Status::Progress(1))
        } else {
            Ok(Status::Progress(0))
        }
    }

    fn prepare_recv_buffers(
        &self,
        pre_id: &mut ulib::ucm::PreparedCmId,
        recv_mrs: &mut Vec<ulib::uverbs::MemoryRegion<u8>>,
    ) -> Result<(Vec<(Handle, usize, usize, i64)>, Vec<RawFd>), ControlPathError> {
        // create 128 receive mrs, post recv requestse and
        // This is safe because even though recv_mr is moved, the backing memfd and
        // mapped memory regions are still there, and nothing of this mr is changed.
        // We also make sure the lifetime of the mr is longer by storing it in the
        // state.
        for _ in 0..128 {
            let recv_mr: ulib::uverbs::MemoryRegion<u8> = pre_id.alloc_msgs(8 * 1024 * 1024)?;
            recv_mrs.push(recv_mr);
        }
        for recv_mr in recv_mrs.iter_mut() {
            let wr_id = recv_mr.as_handle().0 as u64;
            let wr_ctx = WrContext {
                conn_id: pre_id.as_handle(),
                mr_addr: recv_mr.as_ptr().addr(),
            };
            unsafe {
                pre_id.post_recv(recv_mr, .., wr_id)?;
            }
            self.tls
                .state
                .resource()
                .wr_contexts
                .insert(wr_id, wr_ctx)?;
        }
        let mut returned_mrs = Vec::with_capacity(128);
        let mut fds = Vec::with_capacity(128);
        for recv_mr in recv_mrs {
            returned_mrs.push((
                recv_mr.as_handle(),
                recv_mr.as_ptr().addr(),
                recv_mr.len(),
                recv_mr.file_off() as i64,
            ));
            fds.push(recv_mr.memfd().as_raw_fd());
        }
        Ok((returned_mrs, fds))
    }

    async fn check_input_cmd_queue(&mut self) -> Result<Status, ControlPathError> {
        use tokio::sync::mpsc::error::TryRecvError;
        match self.cmd_rx.try_recv() {
            Ok(req) => {
                let result = self.process_cmd(&req).await;
                match result {
                    Ok(res) => self.cmd_tx.send(mrpc::cmd::Completion(Ok(res)))?,
                    Err(e) => self.cmd_tx.send(mrpc::cmd::Completion(Err(e.into())))?,
                }
                Ok(Progress(1))
            }
            Err(TryRecvError::Empty) => Ok(Progress(0)),
            Err(TryRecvError::Disconnected) => Ok(Status::Disconnected),
        }
    }

    async fn process_cmd(
        &mut self,
        req: &mrpc::cmd::Command,
    ) -> Result<mrpc::cmd::CompletionKind, ControlPathError> {
        match req {
            mrpc::cmd::Command::SetTransport(_) => {
                unreachable!();
            }
            // mrpc::cmd::Command::AllocShm(nbytes) => {
            //     log::trace!("AllocShm, nbytes: {}", *nbytes);
            //     let pd = &self.tls.state.resource().default_pds()[0];
            //     let access = ulib::uverbs::AccessFlags::REMOTE_READ
            //         | ulib::uverbs::AccessFlags::REMOTE_WRITE
            //         | ulib::uverbs::AccessFlags::LOCAL_WRITE;
            //     let mr: ulib::uverbs::MemoryRegion<u8> = pd.allocate(*nbytes, access)?;
            //     let returned_mr = interface::returned::MemoryRegion {
            //         handle: mr.inner,
            //         rkey: mr.rkey(),
            //         vaddr: mr.as_ptr() as u64,
            //         map_len: mr.len() as u64,
            //         file_off: mr.file_off,
            //         pd: mr.pd().inner,
            //     };
            //     // store the allocated MRs for later memory address translation
            //     let memfd = mr.memfd().as_raw_fd();
            //     // log::debug!("mr.addr: {:0x}", mr.as_ptr() as usize);
            //     // log::debug!(
            //     //     "mr_table: {:0x?}",
            //     //     self.tls
            //     //         .state
            //     //         .resource()
            //     //         .mr_table
            //     //         .lock()
            //     //         .keys()
            //     //         .copied()
            //     //         .collect::<Vec<usize>>()
            //     // );
            //     self.tls.state.resource().insert_mr(mr)?;
            //     Ok(mrpc::cmd::CompletionKind::AllocShmInternal(
            //         returned_mr,
            //         memfd,
            //     ))
            // }
            mrpc::cmd::Command::Connect(addr) => {
                log::trace!("Connect, addr: {:?}", addr);
                // create CmIdBuilder
                let cq = self.get_or_init_cq();
                let builder = ulib::ucm::CmIdBuilder::new()
                    .set_send_cq(cq)
                    .set_recv_cq(cq)
                    .set_max_send_wr(128)
                    .set_max_recv_wr(128)
                    .resolve_route(addr).await?;
                let mut pre_id = builder.build()?;
                // prepare and post receive buffers
                let mut recv_mrs = Vec::with_capacity(128);
                let (returned_mrs, fds) = self.prepare_recv_buffers(&mut pre_id, &mut recv_mrs)?;
                // connect
                let id = pre_id.connect(None).await?;
                let handle = id.as_handle();

                // insert resources after connection establishment
                self.tls.state.resource().insert_cmid(id, 128)?;
                for recv_mr in recv_mrs {
                    self.tls
                        .state
                        .resource()
                        .recv_mr_table
                        .insert(recv_mr.as_handle(), recv_mr)?;
                }
                Ok(mrpc::cmd::CompletionKind::ConnectInternal(
                    handle,
                    returned_mrs,
                    fds,
                ))
            }
            mrpc::cmd::Command::Bind(addr) => {
                log::trace!("Bind, addr: {:?}", addr);
                // create CmIdBuilder
                let listener = ulib::ucm::CmIdBuilder::new().bind(addr).await?;
                let handle = listener.as_handle();
                self.tls
                    .state
                    .resource()
                    .listener_table
                    .insert(handle, listener)?;
                self.recent_listener_handle.replace(handle);
                Ok(mrpc::cmd::CompletionKind::Bind(handle))
            }
            // mrpc::cmd::Command::NewMappedAddrs(app_vaddrs) => {
            //     // find those existing mrs, and update their app_vaddrs
            //     let mut ret = Vec::new();
            //     for (mr_handle, app_vaddr) in app_vaddrs {
            //         let mr = self.tls.state.resource().recv_mr_table.get(mr_handle)?;
            //         mr.set_app_vaddr(*app_vaddr);
            //         ret.push((mr.as_ptr() as usize, *app_vaddr as usize, mr.len()));
            //     }
            //     Ok(mrpc::cmd::CompletionKind::NewMappedAddrsInternal(ret))
            //     // Err(ControlPathError::NoResponse)
            // }
        }
    }
}
