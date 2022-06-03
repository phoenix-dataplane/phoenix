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
use crate::engine::{future, Engine, EngineLocalStorage, EngineResult, Indicator, EngineRxMessage, Vertex};
use crate::mrpc::marshal::{MessageTemplate, RpcMessage, SgList, ShmBuf, Unmarshal};
use crate::node::Node;
use crate::salloc::region::SharedRegion;
use crate::salloc::state::State as SallocState;
use crate::transport::rdma::ops::Ops;

pub(crate) struct TlStorage {
    pub(crate) ops: Ops,
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

pub(crate) struct RpcAdapterEngine {
    // NOTE(cjr): The drop order here is important. objects in ulib first, objects in transport later.
    pub(crate) state: State,
    pub(crate) odp_mr: Option<ulib::uverbs::MemoryRegion<u8>>,
    pub(crate) tls: Box<TlStorage>,

    pub(crate) salloc: SallocState,

    // shared completion queue model
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
        format!("RcpAdapterEngine, user pid: {:?}", self.state.shared.pid)
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

impl Drop for RpcAdapterEngine {
    fn drop(&mut self) {
        self.state.stop_acceptor(true);
    }
}

impl RpcAdapterEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut work = 0;
            // check input queue
            match self.check_input_queue()? {
                Progress(n) => work += n,
                Status::Disconnected => return Ok(()),
            }

            // check service
            if let Progress(n) = self.check_transport_service()? {
                work += n;
            }

            // check input command queue
            match self.check_input_cmd_queue().await? {
                Progress(n) => work += n,
                Status::Disconnected => return Ok(()),
            }

            // TODO(cjr): check incoming connect request
            // the CmIdListener::get_request() is currently synchronous.
            // need to make it asynchronous and low cost to check.
            // TODO(cjr): move this to another engine and runtime
            self.check_incoming_connection().await?;

            self.indicator.as_ref().unwrap().set_nwork(work);

            future::yield_now().await;
        }
    }
}

impl RpcAdapterEngine {
    fn get_or_init_odp_mr(&mut self) -> &mut ulib::uverbs::MemoryRegion<u8> {
        // this function is not supposed to be called concurrently.
        if self.odp_mr.is_none() {
            // TODO(cjr): we currently by default use the first ibv_context.
            let pd_list = ulib::uverbs::get_default_pds().unwrap();
            let pd = &pd_list[0];
            let odp_mr = self.tls.ops.create_mr_on_demand_paging(&pd.inner).unwrap();
            self.odp_mr = Some(ulib::uverbs::MemoryRegion::<u8>::new(odp_mr).unwrap());
        }
        self.odp_mr.as_mut().unwrap()
    }

    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use tokio::sync::mpsc::error::TryRecvError;
        use ulib::uverbs::SendFlags;

        while let Some(msg) = self.local_buffer.pop_front() {
            // get cmid from conn_id
            let span = info_span!("RpcAdapter check_input_queue: send_msg");
            let _enter = span.enter();

            let msg_ref = unsafe { msg.as_ref() };
            let cmid_handle = msg_ref.conn_id();
            let call_id = msg_ref.call_id();

            let conn_ctx = {
                // let span = info_span!("get conn context");
                // let _enter = span.enter();
                self.state.resource().cmid_table.get(&cmid_handle)?
            };

            if conn_ctx.credit.load(Ordering::Acquire) <= 5 {
                // some random number for now TODO(cjr): update this
                self.local_buffer.push_front(msg);
                break;
            }

            let cmid = &conn_ctx.cmid;

            let sglist = {
                // let span = info_span!("marshal");
                // let _enter = span.enter();
                let sglist = msg_ref.marshal();
                sglist
            };

            // Sender marshals the data (gets an SgList)
            // Sender posts send requests from the SgList
            log::debug!("check_input_queue, sglist: {:0x?}", sglist);
            {
                // let span = info_span!("push_back outstanding_req");
                // let _enter = span.enter();
                if msg_ref.is_request() {
                    conn_ctx.credit.fetch_sub(sglist.0.len(), Ordering::AcqRel);
                    conn_ctx.outstanding_req.lock().push_back(ReqContext {
                        call_id: msg_ref.call_id(),
                        sg_len: sglist.0.len(),
                    });
                }
            }

            // TODO(cjr): credit handle logic for response
            let odp_mr = self.get_or_init_odp_mr();
            {
                // let span = info_span!("post_send: enumerate sglist");
                // let _enter = span.enter();
                for (i, &sge) in sglist.0.iter().enumerate() {
                    // query mr from each sge
                    // let mr = self.salloc.resource().query_mr(sge)?;
                    // let off = sge.ptr - mr.as_ptr() as usize;
                    let off = sge.ptr;
                    if i + 1 < sglist.0.len() {
                        // post send
                        unsafe {
                            // let span = info_span!("post_send");
                            // let _enter = span.enter();
                            cmid.post_send(odp_mr, off..off + sge.len, 0, SendFlags::SIGNALED)?;
                        }
                    } else {
                        // post send with imm
                        tracing::trace!("post_send_imm, len={}", sge.len);
                        let ctx = ((cmid_handle.0 as u64) << 32) | (call_id as u64);
                        unsafe {
                            // let span = info_span!("post_send_with_imm");
                            // let _enter = span.enter();
                            cmid.post_send_with_imm(
                                odp_mr,
                                off..off + sge.len,
                                ctx,
                                SendFlags::SIGNALED,
                                0,
                            )?;
                        }
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
        // log::debug!("unmarshal_and_deliver_up, sgl: {:0x?}", sgl);

        let span = info_span!("unmarshal_and_deliver_up");
        let _enter = span.enter();

        let mut erased = {
            // let span = info_span!("unmarshal MessageTemplateErased");
            // let _enter = span.enter();
            let erased =
                unsafe { MessageTemplateErased::unmarshal(sgl.clone(), &self.salloc.shared) }
                    .unwrap();
            erased
        };

        let meta = &mut unsafe { erased.as_mut() }.meta;
        meta.conn_id = conn_ctx.cmid.as_handle();

        // replenish the credits
        if meta.msg_type == RpcMsgType::Response {
            // let span = info_span!("replenish the credits");
            // let _enter = span.enter();
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
                        let msg = unsafe {
                            // let span = info_span!("unmarshal request message template");
                            // let _enter = span.enter();
                            MessageTemplate::<codegen::HelloRequest>::unmarshal(
                                sgl,
                                &self.salloc.shared,
                            )
                            .unwrap()
                        };
                        MessageTemplate::into_rpc_message(msg)
                    }
                    _ => panic!("unknown func_id: {}, meta: {:?}", meta.func_id, meta),
                }
            }
            RpcMsgType::Response => {
                match meta.func_id {
                    0 => {
                        let msg = unsafe {
                            // let span = info_span!("unmarshal response message template");
                            // let _enter = span.enter();
                            MessageTemplate::<codegen::HelloReply>::unmarshal(
                                sgl,
                                &self.salloc.shared,
                            )
                            .unwrap()
                        };
                        MessageTemplate::into_rpc_message(msg)
                    }
                    _ => panic!("unknown func_id: {}, meta: {:?}", meta.func_id, meta),
                }
            }
        };
        {
            // let span = info_span!("unmarshal_and_deliver_up: rx_outputs send");
            // let _enter = span.enter();
            self.rx_outputs()[0].send(EngineRxMessage::RpcMessage(dyn_msg)).unwrap();
        }
        Ok(Progress(1))
    }

    fn check_transport_service(&mut self) -> Result<Status, DatapathError> {
        // check completion, and replenish some recv requests
        use interface::{WcFlags, WcOpcode, WcStatus};
        let mut comps = Vec::with_capacity(32);
        let cq = self.state.get_or_init_cq();
        cq.poll(&mut comps)?;

        let mut progress = 0;
        for wc in comps {
            match wc.status {
                WcStatus::Success => {
                    let span = info_span!("RpcAdapter check_transport_service: wc polled");
                    let _enter = span.enter();

                    match wc.opcode {
                        WcOpcode::Send => {
                            // send completed, do nothing
                            if wc.wc_flags.contains(WcFlags::WITH_IMM) {
                                let conn_id = Handle((wc.wr_id >> 32) as u32);
                                let call_id = wc.wr_id as u32;
                                tracing::trace!("post_send_imm completed, wr_id={}", wc.wr_id);
                                self.rx_outputs()[0].send(EngineRxMessage::SendCompletion(conn_id, call_id))
                            }
                        }
                        WcOpcode::Recv => {
                            let conn_ctx = {
                                // let span = info_span!("push receiving_sgl");
                                // let _enter = span.enter();
                                let wr_ctx = self.state.resource().wr_contexts.get(&wc.wr_id)?;
                                let cmid_handle = wr_ctx.conn_id;
                                let conn_ctx =
                                    self.state.resource().cmid_table.get(&cmid_handle)?;
                                // received a segment of RPC message
                                let sge = ShmBuf {
                                    ptr: wr_ctx.mr_addr,
                                    len: wc.byte_len as _, // note this byte_len is only valid for
                                                           // recv request
                                };
                                conn_ctx.receiving_sgl.lock().0.push(sge);
                                conn_ctx
                            };

                            if wc.wc_flags.contains(WcFlags::WITH_IMM) {
                                // received an entire RPC message
                                tracing::trace!(
                                    "post_recv received complete message, wr_id={}",
                                    wc.wr_id
                                );
                                use std::ops::DerefMut;
                                let sgl = mem::take(conn_ctx.receiving_sgl.lock().deref_mut());
                                self.unmarshal_and_deliver_up(sgl, Arc::clone(&conn_ctx))?;
                            }
                            // XXX(cjr): only when the upper layer app finish using the data
                            // we can repost the receives
                            let cmid = &conn_ctx.cmid;

                            let recv_mr = self
                                .salloc
                                .resource()
                                .recv_mr_table
                                .get(&Handle(wc.wr_id as u32))?;
                            let mut odp_mr = self.get_or_init_odp_mr();
                            let off = recv_mr.as_ptr().expose_addr();
                            let len = recv_mr.len();

                            unsafe {
                                // TODO(wyj, cjr): This is incorrect. The backend should only
                                // post_recv when the user completes the usage.
                                // let span = info_span!("post_recv");
                                // let _enter = span.enter();
                                cmid.post_recv(&mut odp_mr, off..off + len, wc.wr_id)?;
                            }

                            progress += 1;
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
        Ok(Status::Progress(progress))
    }

    async fn check_incoming_connection(&mut self) -> Result<Status, ControlPathError> {
        let rpc_adapter_id = self.state.rpc_adapter_id;
        let ret = self
            .state
            .resource()
            .pre_cmid_table
            .entry(rpc_adapter_id)
            .or_insert_with(VecDeque::new)
            .pop_front();
        match ret {
            None => Ok(Status::Progress(0)),
            Some(mut pre_id) => {
                // prepare and post receive buffers
                let (returned_mrs, fds) = self.prepare_recv_buffers(&mut pre_id)?;
                // accept
                let id = pre_id.accept(None).await?;
                let handle = id.as_handle();
                // insert resources after connection establishment
                self.state.resource().insert_cmid(id, 128)?;
                // pass these resources back to the user
                let comp = mrpc::cmd::Completion(Ok(
                    mrpc::cmd::CompletionKind::NewConnectionInternal(handle, returned_mrs, fds),
                ));
                self.cmd_tx.send(comp)?;
                Ok(Status::Progress(1))
            }
        }
    }

    fn prepare_recv_buffers(
        &mut self,
        pre_id: &mut ulib::ucm::PreparedCmId,
    ) -> Result<(Vec<(Handle, usize, usize, i64)>, Vec<RawFd>), ControlPathError> {
        // create 128 receive mrs, post recv requestse and
        // This is safe because even though recv_mr is moved, the backing memfd and
        // mapped memory regions are still there, and nothing of this mr is changed.
        // We also make sure the lifetime of the mr is longer by storing it in the
        // state.
        let mut recv_mrs = Vec::new();
        for _ in 0..128 {
            let recv_mr: Arc<SharedRegion> =
                self.salloc.resource().allocate_recv_mr(8 * 1024 * 1024)?;
            recv_mrs.push(recv_mr);
        }
        // for _ in 0..128 {
        //     let recv_mr: ulib::uverbs::MemoryRegion<u8> = pre_id.alloc_msgs(8 * 1024 * 1024)?;
        //     recv_mrs.push(recv_mr);
        // }
        for recv_mr in recv_mrs.iter_mut() {
            let mut odp_mr = self.get_or_init_odp_mr();
            let wr_id = recv_mr.as_handle().0 as u64;
            let wr_ctx = WrContext {
                conn_id: pre_id.as_handle(),
                mr_addr: recv_mr.as_ptr().addr(),
            };
            let off = recv_mr.as_ptr().addr();
            let len = recv_mr.len();
            unsafe {
                pre_id.post_recv(&mut odp_mr, off..off + len, wr_id)?;
            }
            self.state.resource().wr_contexts.insert(wr_id, wr_ctx)?;
        }
        let mut returned_mrs = Vec::with_capacity(128);
        let mut fds = Vec::with_capacity(128);
        for recv_mr in recv_mrs {
            let file_off = 0;
            returned_mrs.push((
                recv_mr.as_handle(),
                recv_mr.as_ptr().addr(),
                recv_mr.len(),
                file_off,
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
            mrpc::cmd::Command::Connect(addr) => {
                log::debug!("Connect, addr: {:?}", addr);
                // create CmIdBuilder
                let cq = self.state.get_or_init_cq();
                let builder = ulib::ucm::CmIdBuilder::new()
                    .set_send_cq(cq)
                    .set_recv_cq(cq)
                    .set_max_send_wr(128)
                    .set_max_recv_wr(128)
                    .resolve_route(addr)
                    .await?;
                let mut pre_id = builder.build()?;
                // prepare and post receive buffers
                let (returned_mrs, fds) = self.prepare_recv_buffers(&mut pre_id)?;
                // connect
                let id = pre_id.connect(None).await?;
                let handle = id.as_handle();

                // insert resources after connection establishment
                self.state.resource().insert_cmid(id, 128)?;
                Ok(mrpc::cmd::CompletionKind::ConnectInternal(
                    handle,
                    returned_mrs,
                    fds,
                ))
            }
            mrpc::cmd::Command::Bind(addr) => {
                log::debug!("Bind, addr: {:?}", addr);
                // create CmIdBuilder
                let listener = ulib::ucm::CmIdBuilder::new().bind(addr).await?;
                let handle = listener.as_handle();
                self.state
                    .resource()
                    .listener_table
                    .insert(handle, (self.state.rpc_adapter_id, listener))?;
                Ok(mrpc::cmd::CompletionKind::Bind(handle))
            }
            mrpc::cmd::Command::RecycleRecvMr(_) => {
                unimplemented!();
            }
        }
    }
}
