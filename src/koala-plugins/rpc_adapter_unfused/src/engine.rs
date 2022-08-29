use std::cell::RefCell;
use std::collections::VecDeque;
use std::mem;
use std::os::unix::prelude::{AsRawFd, RawFd};
use std::pin::Pin;
use std::ptr;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Result;
use fnv::FnvHashMap;
use futures::future::BoxFuture;

use interface::engine::SchedulingMode;
use interface::rpc::{MessageMeta, RpcId, RpcMsgType, TransportStatus};
use interface::{AsHandle, Handle};
use ipc::mrpc::cmd;
use ipc::mrpc::cmd::{ConnectResponse, ReadHeapRegion};
use ipc::rpc_adapter::control_plane;
use mrpc_marshal::{ExcavateContext, SgE, SgList};

use mrpc::unpack::UnpackFromSgE;
use salloc::state::State as SallocState;
use transport_rdma::ops::Ops;

use koala::engine::datapath::message::{
    EngineRxMessage, EngineTxMessage, RpcMessageRx, RpcMessageTx,
};
use koala::engine::datapath::DataPathNode;
use koala::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use koala::envelop::ResourceDowncast;
use koala::impl_vertex_for_engine;
use koala::module::{ModuleCollection, Version};
use koala::storage::{ResourceCollection, SharedStorage};
use koala::{log, tracing};

use crate::ctrl_msg::ControlMessage;

use super::ctrl_buf::ControlMsgBuffer;
use super::pool::BufferSlab;
use super::serialization::SerializationEngine;
use super::state::{ConnectionContext, ReqContext, State, WrContext};
use super::ulib;
use super::{ControlPathError, DatapathError};

pub(crate) const MAX_INLINE_DATA: usize = 128;

thread_local! {
    /// To emulate a thread local storage (TLS). This should be called engine-local-storage (ELS).
    pub(crate) static ELS: RefCell<Option<&'static TlStorage>> = RefCell::new(None);
}

// Must be `Send`.
pub(crate) struct TlStorage {
    pub(crate) ops: Ops,
}

pub(crate) struct RpcAdapterEngine {
    // NOTE(cjr): The drop order here is important. objects in ulib first, objects in transport later.
    pub(crate) state: State,
    pub(crate) odp_mr: Option<ulib::uverbs::MemoryRegion<u8>>,
    pub(crate) tls: Box<TlStorage>,

    // shared completion queue model
    pub(crate) local_buffer: VecDeque<RpcMessageTx>,

    // the number of pending receives that are going on. this can avoid the runtime from sleeping
    pub(crate) pending_recv: usize,

    // records the recv mr usage (a list of recv mr Handle) of each received message (identified by connection handle and call id)
    // if in the future multiple sge are packed into a single recv mr
    // then we only needs to maintain an additional reference counter for each recv mr, i.e., HashMap<Handle, u64>;
    // if in the future recv mr's addr is directly used as wr_id in post_recv,
    // just change Handle here to usize
    pub(crate) recv_mr_usage: FnvHashMap<RpcId, Vec<Handle>>,

    pub(crate) serialization_engine: Option<SerializationEngine>,

    pub(crate) cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Command>,
    pub(crate) cmd_tx: tokio::sync::mpsc::UnboundedSender<cmd::Completion>,
    pub(crate) node: DataPathNode,

    pub(crate) ctrl_buf: ControlMsgBuffer,
    pub(crate) version: u64,

    pub(crate) _mode: SchedulingMode,
    pub(crate) indicator: Indicator,

    // work completion read buffer
    pub(crate) wc_read_buffer: Vec<interface::WorkCompletion>,

    // NOTE: Hold salloc State to prevent early dropping of send heap.
    pub(crate) salloc: SallocState,
}

impl_vertex_for_engine!(RpcAdapterEngine, node);

impl Decompose for RpcAdapterEngine {
    #[inline]
    fn flush(&mut self) -> Result<()> {
        // each call to `check_input_queue()` receives at most one message
        while !self.tx_inputs()[0].is_empty() {
            self.check_input_queue()?;
        }
        Ok(())
    }

    fn decompose(
        self: Box<Self>,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode) {
        // NOTE(wyj): if command/data queue types need to be upgraded
        // then the channels must be recreated
        let mut engine = *self;

        let mut collections = ResourceCollection::with_capacity(13);
        tracing::trace!("dumping RpcAdapterEngine states...");

        let node = unsafe {
            collections.insert("state".to_string(), Box::new(ptr::read(&mut engine.state)));
            collections.insert("tls".to_string(), Box::new(ptr::read(&mut engine.tls)));
            collections.insert("mode".to_string(), Box::new(ptr::read(&mut engine._mode)));
            collections.insert(
                "odp_mr".to_string(),
                Box::new(ptr::read(&mut engine.odp_mr)),
            );
            collections.insert(
                "local_buffer".to_string(),
                Box::new(ptr::read(&mut engine.local_buffer)),
            );
            collections.insert(
                "pending_recv".to_string(),
                Box::new(ptr::read(&mut engine.pending_recv)),
            );
            collections.insert(
                "recv_mr_usage".to_string(),
                Box::new(ptr::read(&mut engine.recv_mr_usage)),
            );
            collections.insert(
                "serialization_engine".to_string(),
                Box::new(ptr::read(&mut engine.serialization_engine)),
            );
            collections.insert(
                "cmd_tx".to_string(),
                Box::new(ptr::read(&mut engine.cmd_tx)),
            );
            collections.insert(
                "cmd_rx".to_string(),
                Box::new(ptr::read(&mut engine.cmd_rx)),
            );
            collections.insert(
                "wc_read_buffer".to_string(),
                Box::new(ptr::read(&mut engine.wc_read_buffer)),
            );
            collections.insert(
                "salloc".to_string(),
                Box::new(ptr::read(&mut engine.salloc)),
            );
            collections.insert(
                "ctrl_buf".to_string(),
                Box::new(ptr::read(&mut engine.ctrl_buf)),
            );
            // don't call the drop function
            ptr::read(&mut engine.node)
        };

        mem::forget(engine);

        (collections, node)
    }
}

impl RpcAdapterEngine {
    pub(crate) unsafe fn restore(
        mut local: ResourceCollection,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
        node: DataPathNode,
        _plugged: &ModuleCollection,
        _prev_version: Version,
    ) -> Result<Self> {
        tracing::trace!("restoring RpcAdapterEngine states...");
        let state = *local
            .remove("state")
            .unwrap()
            .downcast_unchecked::<State>();
        let tls = *local
            .remove("tls")
            .unwrap()
            .downcast_unchecked::<Box<TlStorage>>();
        let mode = *local
            .remove("mode")
            .unwrap()
            .downcast_unchecked::<SchedulingMode>();
        let odp_mr = *local
            .remove("odp_mr")
            .unwrap()
            .downcast_unchecked::<Option<ulib::uverbs::MemoryRegion<u8>>>();
        let local_buffer = *local
            .remove("local_buffer")
            .unwrap()
            .downcast_unchecked::<VecDeque<RpcMessageTx>>();
        let recv_mr_usage = *local
            .remove("recv_mr_usage")
            .unwrap()
            .downcast_unchecked::<FnvHashMap<RpcId, Vec<Handle>>>();
        let serialization_engine = *local
            .remove("serialization_engine")
            .unwrap()
            .downcast_unchecked::<Option<SerializationEngine>>();
        let cmd_tx = *local
            .remove("cmd_tx")
            .unwrap()
            .downcast_unchecked::<tokio::sync::mpsc::UnboundedSender<cmd::Completion>>();
        let cmd_rx = *local
            .remove("cmd_rx")
            .unwrap()
            .downcast_unchecked::<tokio::sync::mpsc::UnboundedReceiver<cmd::Command>>();
        let pending_recv = *local
            .remove("pending_recv")
            .unwrap()
            .downcast_unchecked::<usize>();
        let wc_read_buffer = *local
            .remove("wc_read_buffer")
            .unwrap()
            .downcast_unchecked::<Vec<interface::WorkCompletion>>();
        let salloc = *local
            .remove("salloc")
            .unwrap()
            .downcast_unchecked::<SallocState>();
        let ctrl_buf = *local
            .remove("ctrl_buf")
            .unwrap()
            .downcast_unchecked::<ControlMsgBuffer>();

        let engine = RpcAdapterEngine {
            state,
            odp_mr,
            tls,
            local_buffer,
            pending_recv,
            recv_mr_usage,
            serialization_engine,
            cmd_tx,
            cmd_rx,
            node,
            ctrl_buf,
            version: 1,
            _mode: mode,
            indicator: Default::default(),
            wc_read_buffer,
            salloc,
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

impl Engine for RpcAdapterEngine {
    fn description(self: Pin<&Self>) -> String {
        format!(
            "RcpAdapterEngine, user: {:?}",
            self.get_ref().state.shared.pid
        )
    }

    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    #[inline]
    fn set_els(self: Pin<&mut Self>) {
        let tls = self.get_mut().tls.as_ref() as *const TlStorage;
        // SAFETY: This is fine here because ELS is only used while the engine is running.
        // As long as we do not move out or drop self.tls, we are good.
        ELS.with_borrow_mut(|els| *els = unsafe { Some(&*tls) });
    }

    fn handle_request(
        &mut self,
        request: Vec<u8>,
        _cred: std::os::unix::ucred::UCred,
    ) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        // TODO: send result to userland
        match request {
            control_plane::Request::ListConnection => {
                let mut connections =
                    Vec::with_capacity(self.state.resource().cmid_table.inner().len());
                for conn_ctx in self.state.resource().cmid_table.inner().iter() {
                    let cmid = conn_ctx.data().cmid.inner.handle;
                    let local_addr = conn_ctx.data().cmid.get_local_addr()?;
                    let peer_addr = conn_ctx.data().cmid.get_peer_addr()?;
                    let conn = control_plane::Connection {
                        cmid,
                        local: local_addr,
                        peer: peer_addr,
                    };
                    connections.push(conn);
                }

                for conn in connections {
                    log::info!(
                        "RpcAdapter connection, CmId={:?}, local_addr={:?}, peer_addr={:?}",
                        conn.cmid,
                        conn.local,
                        conn.peer
                    );
                }
            }
        }
        Ok(())
    }
}

impl Drop for RpcAdapterEngine {
    fn drop(&mut self) {
        let this = Pin::new(self);
        let desc = this.as_ref().description();
        log::debug!("{} is being dropped", desc);
        this.get_mut().state.stop_acceptor(true);
        log::debug!("stop acceptor bit set");
    }
}

impl RpcAdapterEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            // let mut timer = crate::timer::Timer::new();
            let mut work = 0;
            // let mut work2 = 0;
            // no work: 10-100ns
            // has work: ~150-180ns each req on avg
            loop {
                // check input queue, no work 10ns, otherwise 250-350ns
                match self.check_input_queue()? {
                    Progress(0) => break,
                    Progress(n) => work += n,
                    Status::Disconnected => return Ok(()),
                }
            }
            // timer.tick();

            // no work: 80-130ns
            // has work: ~320ns each wc on avg
            loop {
                // ibv_poll_cq, no work: 100-150ns, otherwise 400ns
                if let Progress(n) = self.check_transport_service()? {
                    work += n;
                    // work2 += n;
                    if n == 0 {
                        break;
                    }
                }
            }
            // timer.tick();

            if fastrand::usize(..1000) < 1 {
                // check input command queue, ~50ns
                match self.check_input_cmd_queue().await? {
                    Progress(n) => work += n,
                    Status::Disconnected => return Ok(()),
                }
                // timer.tick();

                // TODO(cjr): check incoming connect request, ~200ns
                self.check_incoming_connection().await?;
                // timer.tick();
            }

            // If there's pending receives, there will always be future work to do.
            self.indicator.set_nwork(work + self.pending_recv);

            // log::info!("RpcAdapter mainloop: {} {} {} {}", work - work2, work2, self.pending_recv, timer);
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

    fn send_engine_version(&mut self, conn_ctx: &ConnectionContext) -> Result<(), DatapathError> {
        use ulib::uverbs::SendFlags;

        assert_eq!(self.ctrl_buf.in_use, false);
        let ctrl_ptr = self.ctrl_buf.as_ctrl_msg_ptr();
        let ctrl_msg = unsafe { &mut *ctrl_ptr };
        *ctrl_msg = ControlMessage::ExchangeVersion(self.version);

        let cmid = &conn_ctx.cmid;
        let off = ctrl_ptr.addr();
        let len = mem::size_of::<ControlMessage>();
        let ctx = u64::MAX;

        let odp_mr = self.get_or_init_odp_mr();
        // post send with imm
        tracing::trace!("post_send_imm, control_msg, len={}", len);
        unsafe {
            cmid.post_send_with_imm(odp_mr, off..off + len, ctx, SendFlags::SIGNALED, 0)?;
        }

        // NOTE(wyj): wait for post_send completion
        // loop should be fine for now
        // this is a prototype, and it is not on the fast path
        // (control path)
        while self.ctrl_buf.in_use {
            self.check_transport_service()?;
        }
        
        conn_ctx
            .local_version
            .store(self.version, Ordering::Release);
        Ok(())
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
            self.pending_recv += sglist.0.len() + 1;
            conn_ctx.outstanding_req.lock().push_back(ReqContext {
                call_id,
                sg_len: sglist.0.len() + 1,
            });
        }

        // Sender posts send requests from the SgList
        let ctx = RpcId::new(cmid.as_handle(), call_id).encode_u64();
        let meta_sge = SgE {
            ptr: (meta_ref as *const MessageMeta).expose_addr(),
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
                ctx,
                SendFlags::SIGNALED,
            )?;
        }

        // post the remaining data
        for (i, &sge) in sglist.0.iter().enumerate() {
            let off = sge.ptr;
            if i + 1 < sglist.0.len() {
                // post send
                unsafe {
                    cmid.post_send(odp_mr, off..off + sge.len, ctx, SendFlags::SIGNALED)?;
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
        use koala::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => self.local_buffer.push_back(msg),
                    EngineTxMessage::ReclaimRecvBuf(conn_id, call_ids) => {
                        // let mut timer = crate::timer::Timer::new();
                        let conn_ctx = self.state.resource().cmid_table.get(&conn_id)?;
                        let cmid = &conn_ctx.cmid;
                        // timer.tick();

                        // TODO(cjr): only handle the first element, fix it later
                        for call_id in &call_ids[..1] {
                            let recv_buffer_handles = self
                                .recv_mr_usage
                                .remove(&RpcId(conn_id, *call_id))
                                .expect("invalid WR identifier");
                            self.reclaim_recv_buffers(cmid, &recv_buffer_handles)?;
                        }
                        // timer.tick();
                        // log::info!("ReclaimRecvBuf: {}", timer);
                    }
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

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

            let sglist = if let Some(ref module) = self.serialization_engine {
                module.marshal(meta_ref, msg.addr_backend).unwrap()
            } else {
                panic!("dispatch module not loaded");
            };
            // timer.tick();

            let status = self.send_standard(&conn_ctx, &meta_ref, &sglist)?;

            // timer.tick();
            // log::info!("check_input_queue: {}", timer);
            return Ok(status);
        }

        Ok(Progress(0))
    }

    fn process_control_message(
        &mut self,
        sgl: SgList,
        conn_ctx: &ConnectionContext,
    ) -> Result<(), DatapathError> {
        // currently, control message should only have a single SgE
        assert_eq!(sgl.0.len(), 1);
        let ctrl_msg_ptr = sgl.0[0].ptr as *const ControlMessage;
        let ctrl_msg = unsafe { &*ctrl_msg_ptr };
        match ctrl_msg {
            ControlMessage::ExchangeVersion(ver) => {
                conn_ctx.remote_version.store(*ver, Ordering::Release);
            }
        }
        Ok(())
    }

    fn unmarshal_and_deliver_up(
        &mut self,
        sgl: SgList,
        conn_ctx: Arc<ConnectionContext>,
    ) -> Result<RpcId, DatapathError> {
        // log::debug!("unmarshal_and_deliver_up, sgl: {:0x?}", sgl);

        // let mut timer = crate::timer::Timer::new();

        let mut meta_ptr = unsafe { MessageMeta::unpack(&sgl.0[0]) }.unwrap();
        let meta = unsafe { meta_ptr.as_mut() };
        meta.conn_id = conn_ctx.cmid.as_handle();

        let recv_id = RpcId(meta.conn_id, meta.call_id);

        // timer.tick();
        // replenish the credits
        if meta.msg_type == RpcMsgType::Response {
            let call_id = meta.call_id;
            let mut outstanding_req = conn_ctx.outstanding_req.lock();
            let req_ctx = outstanding_req.pop_front().unwrap();
            assert_eq!(call_id, req_ctx.call_id);
            conn_ctx.credit.fetch_add(req_ctx.sg_len, Ordering::AcqRel);
            self.pending_recv -= req_ctx.sg_len;
            drop(outstanding_req);
        }
        // timer.tick();

        let mut excavate_ctx = ExcavateContext {
            sgl: sgl.0[1..].iter(),
            addr_arbiter: &self.state.resource().addr_map,
        };

        let (addr_app, addr_backend) = if let Some(ref module) = self.serialization_engine {
            module.unmarshal(meta, &mut excavate_ctx).unwrap()
        } else {
            panic!("dispatch module not loaded");
        };
        // timer.tick();

        let msg = RpcMessageRx {
            meta: meta_ptr,
            addr_backend,
            addr_app,
        };

        self.rx_outputs()[0]
            .send(EngineRxMessage::RpcMessage(msg))
            .unwrap();

        // timer.tick();
        // log::info!("unmarshal_and_deliver_up {}", timer);

        Ok(recv_id)
    }

    fn check_transport_service(&mut self) -> Result<Status, DatapathError> {
        // check completion, and replenish some recv requests
        use interface::{WcFlags, WcOpcode, WcStatus};

        let cq = self.state.get_or_init_cq();

        // SAFETY: dp::WorkRequest is Copy and zerocopy
        unsafe {
            self.wc_read_buffer.set_len(0);
        }

        cq.poll(&mut self.wc_read_buffer)?;

        let comps = mem::take(&mut self.wc_read_buffer);

        let mut progress = 0;
        for wc in &comps {
            match wc.status {
                WcStatus::Success => {
                    match wc.opcode {
                        WcOpcode::Send => {
                            // send completed,  do nothing
                            if wc.wc_flags.contains(WcFlags::WITH_IMM) {
                                tracing::trace!("post_send_imm completed, wr_id={}", wc.wr_id);
                                if wc.wr_id == u64::MAX {
                                    self.ctrl_buf.in_use = false;
                                } else {
                                    let rpc_id = RpcId::decode_u64(wc.wr_id);
                                    self.rx_outputs()[0]
                                        .send(EngineRxMessage::Ack(rpc_id, TransportStatus::Success))
                                        .unwrap();
                                }
                            }
                        }
                        WcOpcode::Recv => {
                            let conn_ctx = {
                                let wr_ctx = self.state.resource().wr_contexts.get(&wc.wr_id)?;
                                let cmid_handle = wr_ctx.conn_id;
                                let conn_ctx =
                                    self.state.resource().cmid_table.get(&cmid_handle)?;

                                if conn_ctx.local_version.load(Ordering::Acquire) != self.version {
                                    self.send_engine_version(&conn_ctx)?;
                                }

                                // received a segment of RPC message
                                let sge = SgE {
                                    ptr: wr_ctx.buffer_addr,
                                    len: wc.byte_len as _, // note this byte_len is only valid for
                                                           // recv request
                                };
                                let mut recv_ctx = conn_ctx.receiving_ctx.lock();
                                recv_ctx.sg_list.0.push(sge);
                                recv_ctx.recv_buffer_handles.push(Handle(wc.wr_id as u32));
                                drop(recv_ctx);
                                conn_ctx
                            };

                            if wc.wc_flags.contains(WcFlags::WITH_IMM) {
                                // received an entire RPC message
                                tracing::trace!(
                                    "post_recv received complete message, wr_id={}",
                                    wc.wr_id
                                );
                                // let mut timer = crate::timer::Timer::new();

                                use std::ops::DerefMut;
                                let recv_ctx =
                                    mem::take(conn_ctx.receiving_ctx.lock().deref_mut());

                                if recv_ctx.sg_list.0[0].len == mem::size_of::<ControlMessage>() {
                                    self.process_control_message(recv_ctx.sg_list, &conn_ctx)?;
                                } else {
                                    // timer.tick();
                                    // 200-500ns
                                    let recv_id = self.unmarshal_and_deliver_up(
                                        recv_ctx.sg_list,
                                        Arc::clone(&conn_ctx),
                                    )?;
                                    // timer.tick();

                                    // 60-70ns
                                    // keep them outstanding because they will be used by the user
                                    self.recv_mr_usage
                                        .insert(recv_id, recv_ctx.recv_buffer_handles);
                                }                                    
                                // timer.tick();
                                // log::info!("check_transport_service: {}", timer);
                            }
                            progress += 1;
                        }
                        // The below two are probably errors in impl logic, so assert them
                        WcOpcode::Invalid => panic!("invalid wc: {:?}", wc),
                        _ => panic!("Unhandled wc opcode: {:?}", wc),
                    }
                }
                WcStatus::Error(code) => {
                    log::debug!("wc failed: {:?}", wc);
                    // TODO(cjr): bubble up the error, close the connection, and return an error
                    // to the user.
                    let msg = if let Ok(wr_ctx) = self.state.resource().wr_contexts.get(&wc.wr_id) {
                        // this is a recv operation. don't know the rpc_id
                        let conn_id = wr_ctx.conn_id;
                        EngineRxMessage::RecvError(conn_id, TransportStatus::Error(code))
                    } else {
                        let rpc_id = RpcId::decode_u64(wc.wr_id);
                        EngineRxMessage::Ack(rpc_id, TransportStatus::Error(code))
                    };
                    self.rx_outputs()[0]
                        .send(msg)
                        .unwrap_or_else(|e| panic!("bubble up the error, send failed e: {}", e));
                    // the error is caused by unexpected shutdown of mrpc engine
                }
            }
        }

        self.wc_read_buffer = comps;

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
            let recv_buffer = self.state.resource().recv_buffer_table.get(handle)?;
            let off = recv_buffer.addr();
            let len = recv_buffer.len();

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
                let (read_regions, fds) = self.prepare_recv_buffers(&mut pre_id)?;
                let handle = pre_id.as_handle();
                // move pre_cm_id to staging
                self.state
                    .resource()
                    .staging_pre_cmid_table
                    .insert(handle, pre_id)?;
                // pass these resources back to the user
                let conn_resp = ConnectResponse {
                    conn_handle: handle,
                    read_regions,
                };
                let comp = cmd::Completion(Ok(cmd::CompletionKind::NewConnectionInternal(
                    conn_resp, fds,
                )));
                self.cmd_tx.send(comp)?;
                Ok(Status::Progress(1))
            }
        }
    }

    fn prepare_recv_buffers(
        &mut self,
        pre_id: &mut ulib::ucm::PreparedCmId,
    ) -> Result<(Vec<ReadHeapRegion>, Vec<RawFd>), ControlPathError> {
        // create 128 receive mrs, post recv requests
        let slab = BufferSlab::new(
            128,
            8 * 1024 * 1024,
            8 * 1024 * 1024,
            &self.salloc.addr_mediator,
        )?;

        // post receives
        for _ in 0..128 {
            let mut odp_mr = self.get_or_init_odp_mr();
            // This is fine because we just allocated 128 buffers there
            let recv_buffer = slab.obtain().unwrap();

            let handle = recv_buffer.as_handle();
            let wr_id = handle.0 as u64;
            let wr_ctx = WrContext {
                conn_id: pre_id.as_handle(),
                buffer_addr: recv_buffer.addr(),
            };

            let off = recv_buffer.addr();
            let len = recv_buffer.len();

            unsafe {
                pre_id.post_recv(&mut odp_mr, off..off + len, wr_id)?;
            }
            self.state.resource().wr_contexts.insert(wr_id, wr_ctx)?;
            self.state
                .resource()
                .recv_buffer_table
                .insert(handle, recv_buffer)?;
        }

        let region = slab.storage();
        let read_regions = vec![ReadHeapRegion {
            handle: region.as_handle(),
            addr: region.as_ptr().addr(),
            len: region.len(),
            file_off: 0,
        }];
        let fds = vec![region.memfd().as_raw_fd()];

        // don't forget this
        self.state.resource().recv_buffer_pool.replenish(slab);

        Ok((read_regions, fds))
    }

    async fn check_input_cmd_queue(&mut self) -> Result<Status, ControlPathError> {
        use tokio::sync::mpsc::error::TryRecvError;
        match self.cmd_rx.try_recv() {
            Ok(req) => {
                let result = self.process_cmd(&req).await;
                match result {
                    Ok(res) => self.cmd_tx.send(cmd::Completion(Ok(res)))?,
                    Err(e) => self.cmd_tx.send(cmd::Completion(Err(e.into())))?,
                }
                Ok(Progress(1))
            }
            Err(TryRecvError::Empty) => Ok(Progress(0)),
            Err(TryRecvError::Disconnected) => Ok(Status::Disconnected),
        }
    }

    async fn process_cmd(
        &mut self,
        req: &cmd::Command,
    ) -> Result<cmd::CompletionKind, ControlPathError> {
        match req {
            cmd::Command::SetTransport(_) => {
                unreachable!();
            }
            cmd::Command::Connect(addr) => {
                log::debug!("Connect, addr: {:?}", addr);
                // create CmIdBuilder
                let cq = self.state.get_or_init_cq();
                let builder = ulib::ucm::CmIdBuilder::new()
                    .set_send_cq(cq)
                    .set_recv_cq(cq)
                    .set_max_send_wr(128)
                    .set_max_recv_wr(128)
                    .set_max_inline_data(MAX_INLINE_DATA as u32)
                    .resolve_route(addr)
                    .await?;
                let mut pre_id = builder.build()?;
                // prepare and post receive buffers
                let (read_regions, fds) = self.prepare_recv_buffers(&mut pre_id)?;
                // connect
                let id = pre_id.connect(None).await?;
                let handle = id.as_handle();

                // insert resources after connection establishment
                self.state.resource().insert_cmid(id, 128, self.version)?;

                let conn_ctx = self.state.resource().cmid_table.get(&handle)?;
                self.send_engine_version(&conn_ctx).unwrap();
                // NOTE(wyj): may not be necessary
                // wait for remote side's version
                while conn_ctx.remote_version.load(Ordering::Acquire) == 0 {
                    self.check_transport_service().unwrap();
                }

                let conn_resp = ConnectResponse {
                    conn_handle: handle,
                    read_regions,
                };
                Ok(cmd::CompletionKind::ConnectInternal(conn_resp, fds))
            }
            cmd::Command::Bind(addr) => {
                // create CmIdBuilder
                let listener = ulib::ucm::CmIdBuilder::new().bind(addr).await?;
                let handle = listener.as_handle();
                self.state
                    .resource()
                    .listener_table
                    .insert(handle, (self.state.rpc_adapter_id, listener))?;
                Ok(cmd::CompletionKind::Bind(handle))
            }
            cmd::Command::NewMappedAddrs(conn_handle, app_vaddrs) => {
                for (mr_handle, app_vaddr) in app_vaddrs.iter() {
                    let region = self.state.resource().recv_buffer_pool.find(mr_handle)?;
                    let mr_local_addr = region.as_ptr().expose_addr();
                    let mr_remote_mapped = mrpc_marshal::ShmRecvMr {
                        ptr: *app_vaddr,
                        len: region.len(),
                        align: region.align(),
                    };
                    self.state
                        .resource()
                        .addr_map
                        .insert_addr_map(mr_local_addr, mr_remote_mapped)?;
                }
                // finish the last step to establish a connection
                if let Ok(Some(pre_id)) = self
                    .state
                    .resource()
                    .staging_pre_cmid_table
                    .close_resource(&conn_handle)
                {
                    // accept connection after we get the AddrMap updated
                    let id = Arc::try_unwrap(pre_id).unwrap().accept(None).await?;
                    let handle = id.as_handle();
                    
                    self.state.resource().insert_cmid(id, 128, self.version)?;
                    
                    let conn_ctx = self.state.resource().cmid_table.get(&handle)?;
                    self.send_engine_version(&conn_ctx).unwrap();
                    // NOTE(wyj): may not be necessary
                    // wait for remote side's version
                    while conn_ctx.remote_version.load(Ordering::Acquire) == 0 {
                        self.check_transport_service().unwrap();
                    }
                }
                Ok(cmd::CompletionKind::NewMappedAddrs)
            }
            cmd::Command::UpdateProtosInner(dylib) => {
                log::debug!("Loading dispatch library: {:?}", dylib);
                let module = SerializationEngine::new(dylib)?;
                self.serialization_engine = Some(module);
                Ok(cmd::CompletionKind::UpdateProtos)
            }
            cmd::Command::UpdateProtos(_) => {
                unreachable!();
            }
        }
    }
}
