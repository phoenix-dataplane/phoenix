use std::cell::RefCell;
use std::collections::VecDeque;
use std::mem;
use std::os::unix::prelude::{AsRawFd, RawFd};
use std::pin::Pin;
use std::ptr;
use std::time::Duration;

use anyhow::{anyhow, Result};
use fnv::FnvHashMap;
use futures::future::BoxFuture;

use interface::engine::SchedulingMode;
use interface::rpc::{MessageMeta, RpcId, RpcMsgType, TransportStatus};
use interface::{AsHandle, Handle, MappedAddrStatus, WcOpcode, WcStatus};
use ipc::buf::Range;
use ipc::mrpc::cmd::{ConnectResponse, ReadHeapRegion};
use ipc::tcp_rpc_adapter::control_plane;
use ipc::transport::tcp::dp::Completion;
use mrpc::unpack::UnpackFromSgE;
use mrpc_marshal::{AddressArbiter, SgE, SgList};
use prost::Message;
use salloc::state::State as SallocState;
use transport_tcp::ops::Ops;
use transport_tcp::ApiError;

use koala::engine::datapath::message::{
    EngineRxMessage, EngineTxMessage, RpcMessageRx, RpcMessageTx,
};
use koala::engine::datapath::DataPathNode;
use koala::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use koala::envelop::ResourceDowncast;
use koala::impl_vertex_for_engine;
use koala::module::{ModuleCollection, Version};
use koala::resource::Error as ResourceError;
use koala::storage::{ResourceCollection, SharedStorage};
use koala::{log, tracing};

use crate::private_pool::EncodedRecvBuffer;
use crate::rpc_hello::{HelloReply, HelloRequest};
use crate::serialized_pool::{MessageBuffer, MessageBufferPool, MessageBufferPtr};

use super::get_ops;
use super::pool::BufferSlab;
use super::serialization::SerializationEngine;
use super::state::{ConnectionContext, State};
use super::{ControlPathError, DatapathError};

thread_local! {
    /// To emulate a thread local storage (TLS). This should be called engine-local-storage (ELS).
    pub(crate) static ELS: RefCell<Option<&'static TlStorage>> = RefCell::new(None);
}

pub(crate) struct TlStorage {
    pub(crate) ops: Ops,
}

pub(crate) struct TcpRpcAdapterEngine {
    // NOTE(cjr): The drop order here is important. objects in ulib first, objects in transport later.
    pub(crate) state: State,
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

    pub(crate) serialization_engine: Option<SerializationEngine>,
    pub(crate) encoded_pool: MessageBufferPool,

    pub(crate) node: DataPathNode,
    pub(crate) cmd_rx: tokio::sync::mpsc::UnboundedReceiver<ipc::mrpc::cmd::Command>,
    pub(crate) cmd_tx: tokio::sync::mpsc::UnboundedSender<ipc::mrpc::cmd::Completion>,

    pub(crate) _mode: SchedulingMode,
    pub(crate) indicator: Indicator,
    // pub(crate) start: std::time::Instant,
}

impl_vertex_for_engine!(TcpRpcAdapterEngine, node);

impl Decompose for TcpRpcAdapterEngine {
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
                "local_buffer".to_string(),
                Box::new(ptr::read(&mut engine.local_buffer)),
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
                "salloc".to_string(),
                Box::new(ptr::read(&mut engine.salloc)),
            );
            collections.insert(
                "encoded_pool".to_string(),
                Box::new(ptr::read(&mut engine.encoded_pool)),
            );
            // don't call the drop function
            ptr::read(&mut engine.node)
        };

        mem::forget(engine);

        (collections, node)
    }
}

impl TcpRpcAdapterEngine {
    pub(crate) fn restore(
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
            .downcast::<State>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let tls = *local
            .remove("tls")
            .unwrap()
            .downcast::<Box<TlStorage>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let mode = *local
            .remove("mode")
            .unwrap()
            .downcast::<SchedulingMode>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let local_buffer = *local
            .remove("local_buffer")
            .unwrap()
            .downcast::<VecDeque<RpcMessageTx>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let recv_mr_usage = *local
            .remove("recv_mr_usage")
            .unwrap()
            .downcast::<FnvHashMap<RpcId, Vec<Handle>>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let serialization_engine = *local
            .remove("serialization_engine")
            .unwrap()
            .downcast::<Option<SerializationEngine>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let cmd_tx = *local
            .remove("cmd_tx")
            .unwrap()
            .downcast::<tokio::sync::mpsc::UnboundedSender<ipc::mrpc::cmd::Completion>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let cmd_rx = *local
            .remove("cmd_rx")
            .unwrap()
            .downcast::<tokio::sync::mpsc::UnboundedReceiver<ipc::mrpc::cmd::Command>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let salloc = *local
            .remove("salloc")
            .unwrap()
            .downcast::<SallocState>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let encoded_pool = *local
            .remove("encoded_pool")
            .unwrap()
            .downcast::<MessageBufferPool>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = TcpRpcAdapterEngine {
            state,
            tls,
            local_buffer,
            recv_mr_usage,
            serialization_engine,
            encoded_pool,
            cmd_tx,
            cmd_rx,
            node,
            _mode: mode,
            indicator: Default::default(),
            salloc,
            // start: std::time::Instant::now(),
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

impl Engine for TcpRpcAdapterEngine {
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
                let table = get_ops().state.sock_table.borrow();
                let mut connections = Vec::with_capacity(table.len());
                for (handle, (sock, _status)) in table.iter() {
                    let conn = control_plane::Connection {
                        sock: *handle,
                        local: sock.local_addr()?,
                        peer: sock.peer_addr()?,
                    };
                    connections.push(conn);
                }

                for conn in connections {
                    log::info!(
                        "RpcAdapter connection, Socket={:?}, local_addr={:?}, peer_addr={:?}",
                        conn.sock,
                        conn.local,
                        conn.peer
                    );
                }
            }
        }

        Ok(())
    }
}

impl Drop for TcpRpcAdapterEngine {
    fn drop(&mut self) {
        let this = Pin::new(self);
        let desc = this.as_ref().description();
        log::warn!("{} is being dropped", desc);
    }
}

impl TcpRpcAdapterEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            // let mut timer = koala::timer::Timer::new();

            let mut work = 0;
            let mut nums = Vec::new();

            match self.check_input_queue()? {
                Progress(n) => {
                    work += n;
                    nums.push(n)
                }
                Status::Disconnected => return Ok(()),
            }
            // timer.tick();

            if let Progress(n) = self.check_transport_service()? {
                work += n;
                nums.push(n);
            }
            // timer.tick();

            match self.check_input_cmd_queue()? {
                Progress(n) => {
                    work += n;
                    nums.push(n)
                }
                Status::Disconnected => return Ok(()),
            }

            self.indicator.set_nwork(work);

            // if work > 0 {
            //     log::info!("RpcAdapter mainloop: {:?} {}", nums, timer);
            // }
            future::yield_now().await;
        }
    }
}

impl TcpRpcAdapterEngine {
    fn send_encoded(
        &self,
        conn_ctx: &ConnectionContext,
        msg: MessageBufferPtr,
    ) -> Result<Status, DatapathError> {
        let call_id = unsafe { &*msg.meta_ptr() }.call_id;
        let sock_handle = conn_ctx.sock_handle;
        let ctx = RpcId::new(sock_handle, call_id).encode_u64();

        let offset = msg.0.as_ptr().addr() as u64;
        let len = unsafe { msg.0.as_ref().len() } as u64;
        get_ops().post_send(sock_handle, ctx, Range { offset, len }, 1)?;

        Ok(Progress(1))
    }

    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use koala::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => match msg {
                EngineTxMessage::RpcMessage(msg) => self.local_buffer.push_back(msg),
                EngineTxMessage::ReclaimRecvBuf(conn_id, call_ids) => {
                    let sock_handle = {
                        let table = self.state.conn_table.borrow_mut();
                        let conn_ctx = table.get(&conn_id).ok_or(ResourceError::NotFound)?;
                        conn_ctx.sock_handle
                    };
                    // TODO(cjr): only handle the first element, fix it later
                    for call_id in &call_ids[..1] {
                        let recv_mrs = self
                            .recv_mr_usage
                            .remove(&RpcId(conn_id, *call_id))
                            .expect("invalid WR identifier");
                        self.reclaim_recv_buffers(sock_handle, &recv_mrs[..])?;
                    }
                }
            },
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        while let Some(msg) = self.local_buffer.pop_front() {
            // SAFETY: don't know what kind of UB can be triggered
            let meta_ref = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() };
            let table = self.state.conn_table.borrow_mut();
            let conn_ctx = table
                .get(&meta_ref.conn_id)
                .ok_or(ResourceError::NotFound)?;

            let mut msg_buf_ptr = self
                .encoded_pool
                .obtain(RpcId(meta_ref.conn_id, meta_ref.call_id))
                .expect("MessageBufferPool exhausted");
            let msg_buf = unsafe { msg_buf_ptr.0.as_mut() };
            msg_buf.meta = *meta_ref;

            match meta_ref.msg_type {
                RpcMsgType::Request => match meta_ref.func_id {
                    3687134534u32 => {
                        let msg_ref = unsafe { &*(msg.addr_backend as *mut HelloRequest) };
                        msg_buf.encoded_len = msg_ref.encoded_len();
                        msg_ref.encode(&mut msg_buf.encoded.as_mut_slice()).unwrap();
                    }
                    _ => panic!("unknown func_id: {}, meta {:?}", meta_ref.func_id, meta_ref),
                },
                RpcMsgType::Response => match meta_ref.func_id {
                    3687134534u32 => {
                        let msg_ref = unsafe { &*(msg.addr_backend as *mut HelloReply) };
                        msg_buf.encoded_len = msg_ref.encoded_len();
                        msg_ref.encode(&mut msg_buf.encoded.as_mut_slice()).unwrap();
                    }
                    _ => panic!("unknown func_id: {}, meta {:?}", meta_ref.func_id, meta_ref),
                },
            }

            let status = self.send_encoded(conn_ctx, msg_buf_ptr)?;

            return Ok(status);
        }

        Ok(Progress(0))
    }

    fn reshape_fused_sg_list(sg_list: &mut SgList) {
        use std::ptr::Unique;
        assert_eq!(sg_list.0.len(), 1);

        let msg_buf_ptr = Unique::new(sg_list.0[0].ptr as *mut MessageBuffer).unwrap();
        let msg_buf = unsafe { msg_buf_ptr.as_ref() };

        // modify the first sge in place
        sg_list.0[0].len = mem::size_of::<MessageMeta>();

        let encoded_sge = SgE {
            ptr: msg_buf.encoded().as_ptr().expose_addr(),
            len: msg_buf.encoded().len(),
        };
        sg_list.0.push(encoded_sge);
    }

    fn unmarshal_and_deliver_up(
        &mut self,
        sgl: SgList,
        sock_handle: Handle,
        recv_mr: Handle,
    ) -> RpcId {
        assert_eq!(sgl.0.len(), 2);
        let mut meta_ptr = unsafe { MessageMeta::unpack(&sgl.0[0]) }.unwrap();
        let meta = unsafe { meta_ptr.as_mut() };
        meta.conn_id = sock_handle;

        let recv_id = RpcId(meta.conn_id, meta.call_id);

        let offset = sgl.0[1].ptr;
        let len = sgl.0[1].len;
        let encoded = unsafe { std::slice::from_raw_parts(offset as *const u8, len) };
        let decoded = self
            .state
            .recv_buffer_table
            .borrow()
            .get(&recv_mr)
            .unwrap()
            .addr();

        match meta.msg_type {
            RpcMsgType::Request => match meta.func_id {
                3687134534u32 => {
                    let decoded = unsafe { &mut *(decoded as *mut HelloRequest) };
                    decoded.merge(encoded).unwrap();
                }
                _ => panic!("unknown func_id: {}, meta {:?}", meta.func_id, meta),
            },
            RpcMsgType::Response => match meta.func_id {
                3687134534u32 => {
                    let decoded = unsafe { &mut *(decoded as *mut HelloReply) };
                    decoded.merge(encoded).unwrap();
                }
                _ => panic!("unknown func_id: {}, meta {:?}", meta.func_id, meta),
            },
        }

        let addr_backend = decoded;
        let addr_app = self
            .state
            .resource()
            .addr_map
            .query_app_addr(addr_backend)
            .unwrap();
        let msg = RpcMessageRx {
            meta: meta_ptr,
            addr_backend,
            addr_app,
        };

        self.rx_outputs()[0]
            .send(EngineRxMessage::RpcMessage(msg))
            .unwrap();

        recv_id
    }

    fn process_new_connection(&mut self, handle: &Handle) -> usize {
        if let Ok(_) = (|| -> Result<(), ControlPathError> {
            let (read_regions, fds) = self.prepare_recv_buffers(*handle)?;
            let conn_resp = ConnectResponse {
                conn_handle: *handle,
                read_regions,
            };
            let comp = ipc::mrpc::cmd::Completion(Ok(
                ipc::mrpc::cmd::CompletionKind::NewConnectionInternal(conn_resp, fds),
            ));
            self.cmd_tx.send(comp)?;
            Ok(())
        })() {
            1
        } else {
            0
        }
    }
    fn process_completion(&mut self, wc: &Completion) -> usize {
        match wc.status {
            WcStatus::Success => {
                match wc.opcode {
                    WcOpcode::Send => {
                        if wc.imm != 0 {
                            let rpc_id = RpcId::decode_u64(wc.wr_id);
                            self.encoded_pool.release(rpc_id).unwrap();
                            self.rx_outputs()[0]
                                .send(EngineRxMessage::Ack(rpc_id, TransportStatus::Success))
                                .unwrap();
                        }
                    }
                    WcOpcode::Recv => {
                        let mut table = self.state.conn_table.borrow_mut();
                        let conn_ctx = table.get_mut(&Handle(wc.conn_id as u32));
                        if conn_ctx.is_none() {
                            return 0;
                        }
                        let conn_ctx = conn_ctx.unwrap();

                        // received a segment of RPC message
                        let sge = SgE {
                            ptr: wc.buf.offset as _,
                            len: wc.byte_len as _,
                        };
                        conn_ctx.receiving_ctx.sg_list.0.push(sge);
                        conn_ctx
                            .receiving_ctx
                            .recv_mrs
                            .push(Handle(wc.wr_id as u32));

                        if wc.imm != 0 {
                            // received an entire RPC message
                            let sock_handle = conn_ctx.sock_handle;
                            let mut recv_ctx = mem::take(&mut conn_ctx.receiving_ctx);
                            drop(table);

                            // check if it is an eager message
                            assert_eq!(
                                recv_ctx.sg_list.0.len(),
                                1,
                                "there should be only a single SgE"
                            );

                            Self::reshape_fused_sg_list(&mut recv_ctx.sg_list);

                            let recv_id = self.unmarshal_and_deliver_up(
                                recv_ctx.sg_list,
                                sock_handle,
                                recv_ctx.recv_mrs[0],
                            );

                            // keep them outstanding because they will be used by the user
                            self.recv_mr_usage.insert(recv_id, recv_ctx.recv_mrs);
                        }
                    }
                    // The below two are probably errors in impl logic, so assert them
                    _ => panic!("invalid wc: {:?}", wc),
                }
            }
            WcStatus::Error(code) => {
                // TODO(cjr): bubble up the error, close the connection, and return an error to the user.
                let handle = Handle(wc.conn_id as u32);
                get_ops().state.listener_table.borrow_mut().remove(&handle);
                get_ops().state.sock_table.borrow_mut().remove(&handle);
                get_ops().state.cq_table.borrow_mut().remove(&handle);
                self.state.conn_table.borrow_mut().remove(&handle);
                let msg = if wc.opcode == WcOpcode::Send {
                    let rpc_id = RpcId::decode_u64(wc.wr_id);
                    EngineRxMessage::Ack(rpc_id, TransportStatus::Error(code))
                } else if wc.opcode == WcOpcode::Recv {
                    EngineRxMessage::RecvError(handle, TransportStatus::Error(code))
                } else {
                    panic!("invalid wc: {:?}", wc);
                };
                self.rx_outputs()[0].send(msg).unwrap();
            }
        }
        return 1;
    }

    fn check_transport_service(&mut self) -> Result<Status, DatapathError> {
        let (conns, wcs) = get_ops().poll_io(Duration::from_micros(5))?;

        let mut progress = 0;
        for conn in &conns {
            progress += self.process_new_connection(conn);
        }
        for wc in &wcs {
            progress += self.process_completion(wc);
        }

        // COMMENT(cjr): Progress(0) here is okay for now because we haven't use the progress as
        // any indicator.
        Ok(Status::Progress(progress))
    }

    fn reclaim_recv_buffers(
        &mut self,
        sock_handle: Handle,
        mr_handles: &[Handle],
    ) -> Result<(), DatapathError> {
        for handle in mr_handles {
            let table = self.state.encoded_buffer_table.borrow();
            let recv_buffer = table.get(handle).ok_or(ResourceError::NotFound)?;

            get_ops().post_recv(
                sock_handle,
                handle.0 as _,
                Range {
                    offset: recv_buffer.addr() as _,
                    len: recv_buffer.len() as _,
                },
            )?;
        }
        Ok(())
    }

    fn prepare_recv_buffers(
        &mut self,
        sock_handle: Handle,
    ) -> Result<(Vec<ReadHeapRegion>, Vec<RawFd>), ControlPathError> {
        let slab = BufferSlab::new(
            128,
            8 * 1024 * 1024,
            8 * 1024 * 1024,
            &self.salloc.addr_mediator,
        )?;
        // create 128 receive mrs, post recv requestse and
        for _ in 0..128 {
            let recv_buffer = slab.obtain().unwrap();

            // HelloRequest and HelloReply have the same layout
            let ptr = recv_buffer.addr() as *mut HelloRequest;
            let buf_addr = recv_buffer.addr() + mem::size_of::<HelloRequest>();
            let buf_len = recv_buffer.len() - (buf_addr - recv_buffer.addr());
            let msg = unsafe { &mut *ptr };
            let buf_app_addr = self
                .state
                .resource()
                .addr_map
                .query_app_addr(buf_addr)
                .unwrap();
            msg.name = unsafe {
                mrpc_marshal::shadow::Vec::from_raw_parts(
                    buf_addr as *mut _,
                    buf_app_addr as *mut _,
                    buf_len,
                    buf_len,
                )
            };

            let wr_id = recv_buffer.as_handle().0 as u64;
            // let offset = recv_buffer.addr() as u64;
            // let len = recv_buffer.len() as u64;

            let encoded_buffer = EncodedRecvBuffer::new(8 * 1024 * 1024);
            let offset: u64 = encoded_buffer.addr() as u64;
            let len: u64 = encoded_buffer.len() as u64;
            get_ops().post_recv(sock_handle, wr_id, Range { offset, len })?;
            self.state
                .encoded_buffer_table
                .borrow_mut()
                .insert(recv_buffer.as_handle(), encoded_buffer);
            self.state
                .recv_buffer_table
                .borrow_mut()
                .insert(recv_buffer.as_handle(), recv_buffer);
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

    fn check_input_cmd_queue(&mut self) -> Result<Status, ControlPathError> {
        use tokio::sync::mpsc::error::TryRecvError;
        match self.cmd_rx.try_recv() {
            Ok(req) => {
                let result = self.process_cmd(&req);
                match result {
                    Ok(res) => self.cmd_tx.send(ipc::mrpc::cmd::Completion(Ok(res)))?,
                    Err(e) => self
                        .cmd_tx
                        .send(ipc::mrpc::cmd::Completion(Err(e.into())))?,
                }
                Ok(Progress(1))
            }
            Err(TryRecvError::Empty) => Ok(Progress(0)),
            Err(TryRecvError::Disconnected) => Ok(Status::Disconnected),
        }
    }

    fn process_cmd(
        &mut self,
        req: &ipc::mrpc::cmd::Command,
    ) -> Result<ipc::mrpc::cmd::CompletionKind, ControlPathError> {
        use ipc::mrpc::cmd::{Command, CompletionKind};
        match req {
            Command::SetTransport(_) => {
                unreachable!();
            }
            Command::NewMappedAddrs(sock_handle, app_vaddrs) => {
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

                //Marked socket as addresses mapped
                let mut table = get_ops().state.sock_table.borrow_mut();
                let value = table.get_mut(sock_handle).ok_or(ApiError::NotFound)?;
                value.1 = MappedAddrStatus::Mapped;
                // insert resources after connection establishment
                self.state
                    .conn_table
                    .borrow_mut()
                    .insert(*sock_handle, ConnectionContext::new(*sock_handle));

                Ok(CompletionKind::NewMappedAddrs)
            }
            Command::Connect(addr) => {
                log::debug!("Connect, addr: {:?}", addr);
                let sock_handle = get_ops().connect(addr)?;
                let (read_regions, fds) = self.prepare_recv_buffers(sock_handle)?;
                self.state
                    .conn_table
                    .borrow_mut()
                    .insert(sock_handle, ConnectionContext::new(sock_handle));
                let conn_resp = ConnectResponse {
                    conn_handle: sock_handle,
                    read_regions,
                };
                Ok(CompletionKind::ConnectInternal(conn_resp, fds))
            }
            Command::Bind(addr) => {
                log::debug!("Bind, addr: {:?}", addr);
                let handle = get_ops().bind(addr)?;
                Ok(CompletionKind::Bind(handle))
            }
            Command::UpdateProtosInner(dylib) => {
                log::debug!("Loading dispatch library: {:?}", dylib);
                let module = SerializationEngine::new(dylib)?;
                self.serialization_engine = Some(module);
                Ok(CompletionKind::UpdateProtos)
            }
            Command::UpdateProtos(_) => {
                unreachable!();
            }
        }
    }
}
