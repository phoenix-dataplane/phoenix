use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::mem;
use std::os::unix::prelude::{AsRawFd, RawFd};
use std::ptr;
use std::slice;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use fnv::FnvHashMap;

use interface::engine::SchedulingMode;
use interface::rpc::{MessageMeta, RpcId, RpcMsgType, TransportStatus};
use interface::{AsHandle, Handle};
use ipc::mrpc;
use ipc::mrpc::dp::WrIdentifier;
use mrpc_marshal::{ExcavateContext, RpcMessage, SgE, SgList, ShmRecvMr};
use mrpc_marshal::{MarshalError, UnmarshalError};

use super::state::{ConnectionContext, ReqContext, State, WrContext};
use super::ulib;
use super::{ControlPathError, DatapathError};
use crate::engine::graph::{EngineTxMessage, RpcMessageRx, RpcMessageTx};
use crate::engine::{
    future, Engine, EngineLocalStorage, EngineResult, EngineRxMessage, Indicator, Vertex,
};
use crate::mrpc::meta_unpack::MetaUnpacking;
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

pub(crate) type MarshalFn = fn(&MessageMeta, usize) -> Result<SgList, MarshalError>;
pub(crate) type UnmarshalFn = fn(
    &MessageMeta,
    &mut ExcavateContext<spin::Mutex<BTreeMap<usize, ShmRecvMr>>>,
) -> Result<(usize, usize), UnmarshalError>;

pub(crate) struct RpcAdapterEngine {
    // NOTE(cjr): The drop order here is important. objects in ulib first, objects in transport later.
    pub(crate) state: State,
    pub(crate) odp_mr: Option<ulib::uverbs::MemoryRegion<u8>>,
    pub(crate) tls: Box<TlStorage>,

    pub(crate) salloc: SallocState,

    // shared completion queue model
    pub(crate) local_buffer: VecDeque<RpcMessageTx>,

    // records the recv mr usage (a list of recv mr Handle) of each received message (identified by connection handle and call id)
    // if in the future multiple sge are packed into a single recv mr
    // then we only needs to maintain an additional reference counter for each recv mr, i.e., HashMap<Handle, u64>;
    // if in the future recv mr's addr is directly used as wr_id in post_recv,
    // just change Handle here to usize
    pub(crate) recv_mr_usage: FnvHashMap<RpcId, Vec<Handle>>,

    pub(crate) dispatch_dylib: libloading::Library,

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
        format!("RcpAdapterEngine, user: {:?}", self.state.shared.pid)
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
        let desc = self.description().to_owned();
        log::warn!("{} is being dropped", desc);
        self.state.stop_acceptor(true);
    }
}

impl RpcAdapterEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            // let mut timer = crate::timer::Timer::new();
            let mut work = 0;
            // check input queue, ~100ns
            match self.check_input_queue()? {
                Progress(n) => work += n,
                Status::Disconnected => return Ok(()),
            }
            // timer.tick();

            // check service, ~350-600ns
            if let Progress(n) = self.check_transport_service()? {
                work += n;
            }
            // timer.tick();

            // check input command queue, ~50ns
            match self.check_input_cmd_queue().await? {
                Progress(n) => work += n,
                Status::Disconnected => return Ok(()),
            }
            // timer.tick();

            // TODO(cjr): check incoming connect request, ~200ns
            self.check_incoming_connection().await?;
            // timer.tick();

            self.indicator.as_ref().unwrap().set_nwork(work);

            // log::info!("RpcAdapter mainloop: {}", timer);
            future::yield_now().await;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RpcStrategy {
    /// The entire message is encapuslated into one message, transmitted with one send/recv
    Fused,
    /// The message is marshaled into an SgList, and transmitted with multiple send/recv operations.
    Standard,
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

    #[inline]
    fn choose_strategy(sglist: &SgList) -> RpcStrategy {
        // See if the total length can fit into a meta buffer
        let nbytes_lens_and_value: usize = sglist
            .0
            .iter()
            .map(|sge| mem::size_of::<u32>() + sge.len)
            .sum();
        if nbytes_lens_and_value < MetaBuffer::capacity() {
            RpcStrategy::Fused
        } else {
            RpcStrategy::Standard
        }
    }

    fn send_fused(
        &mut self,
        conn_ctx: &ConnectionContext,
        mut meta_buf_ptr: MetaBufferPtr,
        sglist: &SgList,
    ) -> Result<Status, DatapathError> {
        use ulib::uverbs::SendFlags;

        let call_id = unsafe { &*meta_buf_ptr.as_meta_ptr() }.call_id;
        let msg_type = unsafe { &*meta_buf_ptr.as_meta_ptr() }.msg_type;
        let cmid = &conn_ctx.cmid;
        let ctx = RpcId::new(cmid.as_handle(), call_id).encode_u64();

        // TODO(cjr): XXX, this credit implementation has big flaws
        if msg_type == RpcMsgType::Request {
            conn_ctx.credit.fetch_sub(1, Ordering::AcqRel);
            conn_ctx
                .outstanding_req
                .lock()
                .push_back(ReqContext { call_id, sg_len: 1 });
        }

        let off = meta_buf_ptr.0.as_ptr().addr();
        let meta_buf = unsafe { meta_buf_ptr.0.as_mut() };

        // write the lens to MetaBuffer
        meta_buf.num_sge = sglist.0.len() as u32;

        let mut value_len = 0;
        let lens_buf = meta_buf.length_delimited.as_mut_ptr().cast::<u32>();
        let value_buf = unsafe { lens_buf.add(sglist.0.len()).cast::<u8>() };

        for (i, sge) in sglist.0.iter().enumerate() {
            // SAFETY: we have done sanity check before in choose_strategy
            unsafe { lens_buf.add(i).write(sge.len as u32) };
            unsafe {
                ptr::copy_nonoverlapping(sge.ptr as *mut u8, value_buf.add(value_len), sge.len);
            }
            value_len += sge.len;
        }

        let odp_mr = self.get_or_init_odp_mr();

        // post send with imm
        tracing::trace!("post_send_imm, len={}", meta_buf.header_len() + value_len);
        unsafe {
            cmid.post_send_with_imm(
                odp_mr,
                off..off + meta_buf.header_len() + value_len,
                ctx,
                SendFlags::SIGNALED,
                0,
            )?;
        }

        Ok(Progress(1))
    }

    fn send_standard(
        &mut self,
        conn_ctx: &ConnectionContext,
        meta_ref: &MessageMeta,
        sglist: &SgList,
    ) -> Result<Status, DatapathError> {
        use ulib::uverbs::SendFlags;

        let call_id = meta_ref.call_id;
        let cmid = &conn_ctx.cmid;

        // TODO(cjr): XXX, this credit implementation has some issues
        log::debug!("check_input_queue, sglist: {:0x?}", sglist);
        if meta_ref.msg_type == RpcMsgType::Request {
            conn_ctx
                .credit
                .fetch_sub(sglist.0.len() + 1, Ordering::AcqRel);
            conn_ctx.outstanding_req.lock().push_back(ReqContext {
                call_id,
                sg_len: sglist.0.len() + 1,
            });
        }

        // Sender posts send requests from the SgList
        let ctx = RpcId::new(cmid.as_handle(), call_id).encode_u64();
        let meta_sge = SgE {
            ptr: (meta_ref as *const MessageMeta).addr(),
            len: mem::size_of::<MessageMeta>(),
        };

        // TODO(cjr): credit handle logic for response
        let odp_mr = self.get_or_init_odp_mr();
        // timer.tick();

        // post send message meta
        unsafe {
            cmid.post_send(
                odp_mr,
                meta_sge.ptr..meta_sge.ptr + meta_sge.len,
                0,
                SendFlags::SIGNALED,
            )?;
        }

        // post the remaining data
        for (i, &sge) in sglist.0.iter().enumerate() {
            let off = sge.ptr;
            if i + 1 < sglist.0.len() {
                // post send
                unsafe {
                    cmid.post_send(odp_mr, off..off + sge.len, 0, SendFlags::SIGNALED)?;
                }
            } else {
                // post send with imm
                tracing::trace!("post_send_imm, len={}", sge.len);
                unsafe {
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

        Ok(Progress(1))
    }

    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use crate::engine::graph::TryRecvError;
        use crate::mrpc::codegen;

        while let Some(msg) = self.local_buffer.pop_front() {
            // SAFETY: don't know what kind of UB can be triggered
            let meta_ref = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() };
            let cmid_handle = meta_ref.conn_id;

            // get cmid from conn_id
            let conn_ctx = self.state.resource().cmid_table.get(&cmid_handle)?;

            if conn_ctx.credit.load(Ordering::Acquire) <= 5 {
                // some random number for now TODO(cjr): update this
                self.local_buffer.push_front(msg);
                break;
            }
            // let mut timer = crate::timer::Timer::new();

            let cmid = &conn_ctx.cmid;

            let sglist = unsafe {
                let dispatch_func: libloading::Symbol<MarshalFn> =
                    self.dispatch_dylib.get(b"marshal").unwrap();
                dispatch_func(meta_ref, msg.addr_backend).unwrap()
            };
            // timer.tick();

            // TODO(cjr): Examine the SgList and optimize for small messages
            let status = match Self::choose_strategy(&sglist) {
                RpcStrategy::Fused => self.send_fused(&conn_ctx, msg.meta_buf_ptr, &sglist)?,
                RpcStrategy::Standard => self.send_standard(&conn_ctx, meta_ref, &sglist)?,
            };

            // timer.tick();
            // log::info!("check_input_queue: {}", timer);
            return Ok(status);
        }

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => match msg {
                EngineTxMessage::RpcMessage(msg) => self.local_buffer.push_back(msg),
                EngineTxMessage::ReclaimRecvBuf(conn_id, call_ids) => {
                    // let mut timer = crate::timer::Timer::new();
                    let conn_ctx = self.state.resource().cmid_table.get(&conn_id)?;
                    let cmid = &conn_ctx.cmid;
                    // timer.tick();

                    for call_id in call_ids {
                        let recv_mrs = self
                            .recv_mr_usage
                            .remove(&RpcId(conn_id, call_id))
                            .expect("invalid WR identifier");
                        self.reclaim_recv_buffers(cmid, &recv_mrs[..])?;
                    }
                    // timer.tick();
                    // log::info!("ReclaimRecvBuf: {}", timer);
                }
            },
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }
        Ok(Progress(0))
    }

    fn reshape_fused_sg_list(sg_list: &mut SgList) {
        use std::ptr::Unique;

        assert_eq!(sg_list.0.len(), 1);
        // modify the first sge in place
        sg_list.0[0].len = mem::size_of::<MessageMeta>();

        let meta_buf_ptr = Unique::new(sg_list.0[0].ptr as *mut MetaBuffer).unwrap();
        let meta_buf = unsafe { meta_buf_ptr.as_ref() };

        let num_sge = meta_buf.num_sge as usize;
        let lens_buf = unsafe {
            slice::from_raw_parts(meta_buf.length_delimited.as_ptr().cast::<u32>(), num_sge)
        };
        let value_buf_offset = num_sge * mem::size_of::<u32>();
        for i in 0..num_sge {
            sg_list.0.push(SgE {
                ptr: value_buf_offset + meta_buf.length_delimited.as_ptr().addr(),
                len: lens_buf[i] as usize,
            });
        }
    }

    fn unmarshal_and_deliver_up(
        &mut self,
        sgl: SgList,
        conn_ctx: Arc<ConnectionContext>,
    ) -> Result<RpcId, DatapathError> {
        // log::debug!("unmarshal_and_deliver_up, sgl: {:0x?}", sgl);

        let mut meta_ptr = unsafe { MessageMeta::unpack(&sgl.0[0]) }.unwrap();
        let meta = unsafe { meta_ptr.as_mut() };
        meta.conn_id = conn_ctx.cmid.as_handle();

        let recv_id = RpcId(meta.conn_id, meta.call_id);

        // replenish the credits
        if meta.msg_type == RpcMsgType::Response {
            let call_id = meta.call_id;
            let mut outstanding_req = conn_ctx.outstanding_req.lock();
            let req_ctx = outstanding_req.pop_front().unwrap();
            assert_eq!(call_id, req_ctx.call_id);
            conn_ctx.credit.fetch_add(req_ctx.sg_len, Ordering::AcqRel);
            drop(outstanding_req);
        }

        let mut excavate_ctx = ExcavateContext {
            sgl: sgl.0[1..].iter(),
            salloc: &self.salloc.shared.resource.recv_mr_addr_map,
        };

        let (addr_app, addr_backend) = unsafe {
            let dispatch_func: libloading::Symbol<UnmarshalFn> =
                self.dispatch_dylib.get(b"unmarshal").unwrap();
            dispatch_func(meta, &mut excavate_ctx).unwrap()
        };
        // timer.tick();

        let msg = RpcMessageRx {
            meta: meta_ptr,
            addr_backend,
            addr_app,
        };
        // timer.tick();

        self.rx_outputs()[0]
            .send(EngineRxMessage::RpcMessage(msg))
            .unwrap();

        // timer.tick();
        // log::info!("dura1: {:?}, dura2: {:?}", dura1, dura2 - dura1);
        // log::info!("unmarshal_and_deliver_up {}", timer);

        Ok(recv_id)
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
                    match wc.opcode {
                        WcOpcode::Send => {
                            // send completed,  do nothing
                            if wc.wc_flags.contains(WcFlags::WITH_IMM) {
                                tracing::trace!("post_send_imm completed, wr_id={}", wc.wr_id);
                                let rpc_id = RpcId::decode_u64(wc.wr_id);
                                self.rx_outputs()[0]
                                    .send(EngineRxMessage::Ack(rpc_id, TransportStatus::Success))
                                    .unwrap();
                            }
                        }
                        WcOpcode::Recv => {
                            let conn_ctx = {
                                let wr_ctx = self.state.resource().wr_contexts.get(&wc.wr_id)?;
                                let cmid_handle = wr_ctx.conn_id;
                                let conn_ctx =
                                    self.state.resource().cmid_table.get(&cmid_handle)?;
                                // received a segment of RPC message
                                let sge = SgE {
                                    ptr: wr_ctx.mr_addr,
                                    len: wc.byte_len as _, // note this byte_len is only valid for
                                                           // recv request
                                };
                                let mut recv_ctx = conn_ctx.receiving_ctx.lock();
                                recv_ctx.sg_list.0.push(sge);
                                recv_ctx.recv_mrs.push(Handle(wc.wr_id as u32));
                                drop(recv_ctx);
                                conn_ctx
                            };

                            if wc.wc_flags.contains(WcFlags::WITH_IMM) {
                                // received an entire RPC message
                                tracing::trace!(
                                    "post_recv received complete message, wr_id={}",
                                    wc.wr_id
                                );
                                use std::ops::DerefMut;
                                let mut recv_ctx =
                                    mem::take(conn_ctx.receiving_ctx.lock().deref_mut());

                                // check if it is an eager message
                                if recv_ctx.sg_list.0.len() == 1 {
                                    // got an eager message
                                    Self::reshape_fused_sg_list(&mut recv_ctx.sg_list);
                                }

                                let recv_id = self.unmarshal_and_deliver_up(
                                    recv_ctx.sg_list,
                                    Arc::clone(&conn_ctx),
                                )?;

                                // insert mr usages
                                self.recv_mr_usage.insert(recv_id, recv_ctx.recv_mrs);
                            }
                            progress += 1;
                        }
                        // The below two are probably errors in impl logic, so assert them
                        WcOpcode::Invalid => panic!("invalid wc: {:?}", wc),
                        _ => panic!("Unhandled wc opcode: {:?}", wc),
                    }
                }
                WcStatus::Error(code) => {
                    log::warn!("wc failed: {:?}", wc);
                    // TODO(cjr): bubble up the error, close the connection, and return an error
                    // to the user.
                    let rpc_id = RpcId::decode_u64(wc.wr_id);
                    self.rx_outputs()[0]
                        .send(EngineRxMessage::Ack(rpc_id, TransportStatus::Error(code)))
                        .unwrap();
                }
            }
        }
        // COMMENT(cjr): Progress(0) here is okay for now because we haven't use the progress as
        // any indicator.
        Ok(Status::Progress(progress))
    }

    fn reclaim_recv_buffers(
        &mut self,
        cmid: &ulib::ucm::CmId,
        mr_handles: &[Handle],
    ) -> Result<(), DatapathError> {
        for handle in mr_handles {
            let recv_mr = self.salloc.resource().recv_mr_table.get(handle)?;
            let off = recv_mr.as_ptr().expose_addr();
            let len = recv_mr.len();

            let mut odp_mr = self.get_or_init_odp_mr();
            unsafe {
                cmid.post_recv(&mut odp_mr, off..off + len, handle.0 as u64)?;
            }
        }
        Ok(())
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
        }
    }
}
