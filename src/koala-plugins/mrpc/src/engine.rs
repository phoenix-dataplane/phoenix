use std::mem;
use std::path::PathBuf;
use std::pin::Pin;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;

use interface::engine::SchedulingMode;
use interface::rpc::{MessageErased, RpcId};
use ipc::mrpc::{cmd, control_plane, dp};

use koala::engine::datapath::message::{EngineRxMessage, EngineTxMessage, RpcMessageTx};
use koala::engine::datapath::meta_pool::MetaBufferPool;
use koala::engine::datapath::DataPathNode;
use koala::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use koala::envelop::ResourceDowncast;
use koala::impl_vertex_for_engine;
use koala::module::{ModuleCollection, Version};
use koala::storage::{ResourceCollection, SharedStorage};
use koala::tracing;

use super::builder::build_serializer_lib;
use super::module::CustomerType;
use super::state::State;
use super::{DatapathError, Error};

pub struct MrpcEngine {
    pub(crate) _state: State,

    pub(crate) customer: CustomerType,
    pub(crate) cmd_tx: tokio::sync::mpsc::UnboundedSender<cmd::Command>,
    pub(crate) cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Completion>,

    pub(crate) node: DataPathNode,

    // mRPC private buffer pools
    /// Buffer pool for meta and eager message
    pub(crate) meta_buf_pool: MetaBufferPool,

    pub(crate) _mode: SchedulingMode,

    pub(crate) dispatch_build_cache: PathBuf,

    pub(crate) transport_type: Option<control_plane::TransportType>,

    pub(crate) indicator: Indicator,
    pub(crate) wr_read_buffer: Vec<dp::WorkRequest>,
}

impl_vertex_for_engine!(MrpcEngine, node);

impl Decompose for MrpcEngine {
    #[inline]
    fn flush(&mut self) -> Result<()> {
        // mRPC engine has a single receiver on data path,
        // i.e., rx_inputs()[0]
        // each call to `check_input_queue()` processes at most one message
        while !self.rx_inputs()[0].is_empty() {
            self.check_input_queue()?;
        }
        Ok(())
    }

    fn decompose(
        self: Box<Self>,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode) {
        // NOTE(wyj): if command queue types need to be upgraded
        // then the channels must be recreated
        // if the DataPathNode is not flushed,
        // upgraded engine must properly handle the remaining messages in the queues
        let engine = *self;

        let mut collections = ResourceCollection::with_capacity(10);
        tracing::trace!("dumping MrpcEngine states...");
        collections.insert("customer".to_string(), Box::new(engine.customer));
        collections.insert("mode".to_string(), Box::new(engine._mode));
        collections.insert("state".to_string(), Box::new(engine._state));
        collections.insert("cmd_tx".to_string(), Box::new(engine.cmd_tx));
        collections.insert("cmd_rx".to_string(), Box::new(engine.cmd_rx));
        collections.insert("meta_buf_pool".to_string(), Box::new(engine.meta_buf_pool));
        collections.insert(
            "dispatch_build_cache".to_string(),
            Box::new(engine.dispatch_build_cache),
        );
        collections.insert(
            "transport_type".to_string(),
            Box::new(engine.transport_type),
        );
        collections.insert(
            "wr_read_buffer".to_string(),
            Box::new(engine.wr_read_buffer),
        );
        (collections, engine.node)
    }
}

impl MrpcEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
        node: DataPathNode,
        _plugged: &ModuleCollection,
        _prev_version: Version,
    ) -> Result<Self> {
        tracing::trace!("restoring MrpcEngine states...");

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
        let state = *local
            .remove("state")
            .unwrap()
            .downcast::<State>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let cmd_tx = *local
            .remove("cmd_tx")
            .unwrap()
            .downcast::<tokio::sync::mpsc::UnboundedSender<cmd::Command>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let cmd_rx = *local
            .remove("cmd_rx")
            .unwrap()
            .downcast::<tokio::sync::mpsc::UnboundedReceiver<cmd::Completion>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let meta_buf_pool = *local
            .remove("meta_buf_pool")
            .unwrap()
            .downcast::<MetaBufferPool>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let dispatch_build_cache = *local
            .remove("dispatch_build_cache")
            .unwrap()
            .downcast::<PathBuf>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let transport_type = *local
            .remove("transport_type")
            .unwrap()
            .downcast::<Option<control_plane::TransportType>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let wr_read_buffer = *local
            .remove("wr_read_buffer")
            .unwrap()
            .downcast::<Vec<dp::WorkRequest>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = MrpcEngine {
            _state: state,
            customer,
            cmd_tx,
            cmd_rx,
            node,
            meta_buf_pool,
            _mode: mode,
            dispatch_build_cache,
            transport_type,
            indicator: Default::default(),
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

impl Engine for MrpcEngine {
    fn description(self: Pin<&Self>) -> String {
        format!("MrpcEngine, todo show more information")
    }

    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }
}

impl MrpcEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            // let mut timer = crate::timer::Timer::new();
            let mut nwork = 0;

            // no work 80ns
            // has work: <1us for a batch of 30
            loop {
                // no work: 40ns
                if let Progress(n) = self.check_customer()? {
                    nwork += n;
                    if n == 0 {
                        break;
                    }
                }
            }
            // timer.tick();

            // no work: 20ns
            // has work: <2us for a batch of 30
            loop {
                match self.check_input_queue()? {
                    Progress(0) => break,
                    Progress(n) => nwork += n,
                    Status::Disconnected => break,
                }
            }
            // timer.tick();

            if fastrand::usize(..1000) < 1 {
                // 80-100ns, sometimes 200ns
                if let Status::Disconnected = self.check_cmd().await? {
                    break;
                }
                // timer.tick();

                // 50ns
                self.check_input_cmd_queue()?;
                // timer.tick();
            }

            self.indicator.set_nwork(nwork);
            // log::info!("mrpc mainloop: {} {}", nwork, timer);

            future::yield_now().await;
        }

        self.wait_outstanding_complete().await?;
        return Ok(());
    }
}

impl MrpcEngine {
    // we need to wait for RpcAdapter engine to finish outstanding send requests
    // (whether successful or not), to release message meta pool and shutdown mRPC engine.
    // However, we cannot indefinitely wait for it in case of wc errors.
    async fn wait_outstanding_complete(&mut self) -> Result<(), DatapathError> {
        use koala::engine::datapath::TryRecvError;
        while !self.meta_buf_pool.is_full() {
            match self.rx_inputs()[0].try_recv() {
                Ok(msg) => match msg {
                    EngineRxMessage::Ack(rpc_id, _status) => {
                        // release the buffer whatever the status is.
                        self.meta_buf_pool.release(rpc_id)?;
                    }
                    EngineRxMessage::RpcMessage(_) => {}
                    EngineRxMessage::RecvError(..) => {}
                },
                Err(TryRecvError::Disconnected) => return Ok(()),
                Err(TryRecvError::Empty) => {}
            }
            future::yield_now().await;
        }
        Ok(())
    }

    async fn check_cmd(&mut self) -> Result<Status, Error> {
        match self.customer.try_recv_cmd() {
            // handle request
            Ok(req) => {
                let result = self.process_cmd(&req).await;
                match result {
                    Ok(Some(res)) => self.customer.send_comp(cmd::Completion(Ok(res)))?,
                    Ok(None) => return Ok(Progress(0)),
                    Err(e) => self.customer.send_comp(cmd::Completion(Err(e.into())))?,
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

    fn create_transport(&mut self, transport_type: control_plane::TransportType) {
        self.transport_type = Some(transport_type);
    }

    async fn process_cmd(
        &mut self,
        req: &cmd::Command,
    ) -> Result<Option<cmd::CompletionKind>, Error> {
        use ipc::mrpc::cmd::{Command, CompletionKind};
        match req {
            Command::SetTransport(transport_type) => {
                if self.transport_type.is_some() {
                    Err(Error::TransportType)
                } else {
                    self.create_transport(*transport_type);
                    Ok(Some(CompletionKind::SetTransport))
                }
            }
            Command::Connect(addr) => {
                self.cmd_tx.send(Command::Connect(*addr)).unwrap();
                Ok(None)
            }
            Command::Bind(addr) => {
                self.cmd_tx.send(Command::Bind(*addr)).unwrap();
                Ok(None)
            }
            Command::NewMappedAddrs(conn_handle, app_vaddrs) => {
                self.cmd_tx
                    .send(Command::NewMappedAddrs(*conn_handle, app_vaddrs.clone()))
                    .unwrap();
                Ok(None)
            }
            Command::UpdateProtos(protos) => {
                let dylib_path =
                    build_serializer_lib(protos.clone(), self.dispatch_build_cache.clone())?;
                self.cmd_tx
                    .send(Command::UpdateProtosInner(dylib_path))
                    .unwrap();
                Ok(None)
            }
            Command::UpdateProtosInner(_) => {
                panic!("UpdateProtosInner is only used in backend")
            }
        }
    }

    fn check_customer(&mut self) -> Result<Status, DatapathError> {
        use dp::WorkRequest;
        let buffer_cap = self.wr_read_buffer.capacity();
        // let mut timer = crate::timer::Timer::new();

        // 15ns
        // Fetch available work requests. Copy them into a buffer.
        let max_count = buffer_cap.min(self.customer.get_avail_wc_slots()?);
        if max_count == 0 {
            return Ok(Progress(0));
        }
        // timer.tick();

        // 300-10us, mostly 300ns (Vec::with_capacity())
        let mut count = 0;
        // self.wr_read_buffer.clear();
        // SAFETY: dp::WorkRequest is Copy and zerocopy
        unsafe {
            self.wr_read_buffer.set_len(0);
        }

        // timer.tick();

        // 60-150ns
        self.customer
            .dequeue_wr_with(|ptr, read_count| unsafe {
                // TODO(cjr): max_count <= read_count always holds
                count = max_count.min(read_count);
                for i in 0..count {
                    self.wr_read_buffer
                        .push(ptr.add(i).cast::<WorkRequest>().read());
                }
                count
            })
            .unwrap_or_else(|e| panic!("check_customer: {}", e));

        // Process the work requests.
        // timer.tick();

        // no work: 10ns
        // has work: 100-400ns
        let buffer = mem::take(&mut self.wr_read_buffer);

        for wr in &buffer {
            let ret = self.process_dp(wr);
            match ret {
                Ok(()) => {}
                Err(e) => {
                    self.wr_read_buffer = buffer;
                    // TODO(cjr): error handling
                    return Err(e);
                }
            }
        }

        self.wr_read_buffer = buffer;

        // timer.tick();
        // log::info!("check_customer: {} {}", count, timer);

        Ok(Progress(count))
    }

    fn process_dp(&mut self, req: &dp::WorkRequest) -> Result<(), DatapathError> {
        use dp::WorkRequest;

        match req {
            WorkRequest::Call(erased) | WorkRequest::Reply(erased) => {
                // let mut timer = crate::timer::Timer::new();

                // 1300ns, even if the tracing level is filtered shit!!!!!!
                tracing::trace!(
                    "mRPC engine got a message from App, call_id: {}",
                    erased.meta.call_id
                );

                // timer.tick();

                // construct message meta on heap
                let rpc_id = RpcId::new(erased.meta.conn_id, erased.meta.call_id, 0);
                let meta_buf_ptr = self
                    .meta_buf_pool
                    .obtain(rpc_id)
                    .expect("MessageMeta pool exhausted");

                // timer.tick();

                // copy the meta
                unsafe {
                    std::ptr::write(meta_buf_ptr.as_meta_ptr(), erased.meta);
                }

                let msg = RpcMessageTx {
                    meta_buf_ptr,
                    addr_backend: erased.shm_addr_backend,
                };

                // timer.tick();

                // if access to message's data fields are desired,
                // typed message can be conjured up here via matching func_id
                self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;

                // timer.tick();
                // log::info!("process_dp call/reply: {}", timer);
            }
            WorkRequest::ReclaimRecvBuf(conn_id, msg_call_ids) => {
                // let mut timer = crate::timer::Timer::new();

                // 10-40ns, mostly 10ns
                self.tx_outputs()[0]
                    .send(EngineTxMessage::ReclaimRecvBuf(*conn_id, *msg_call_ids))?;

                // timer.tick();
                // log::info!("process_dp reclaim recv buf: {}", timer);
            }
        }
        Ok(())
    }

    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use koala::engine::datapath::TryRecvError;
        match self.rx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineRxMessage::RpcMessage(msg) => {
                        // let mut timer = crate::timer::Timer::new();
                        let meta = unsafe { *msg.meta.as_ref() };
                        tracing::trace!(
                            "mRPC engine send message to App, call_id={}",
                            meta.call_id
                        );

                        let erased = MessageErased {
                            meta,
                            shm_addr_app: msg.addr_app,
                            shm_addr_backend: msg.addr_backend,
                        };
                        // timer.tick();

                        // the following operation takes around 100ns
                        let mut sent = false;
                        while !sent {
                            self.customer.enqueue_wc_with(|ptr, _count| unsafe {
                                sent = true;
                                ptr.cast::<dp::Completion>()
                                    .write(dp::Completion::Incoming(erased));
                                1
                            })?;
                        }

                        // timer.tick();
                        // log::info!("MrpcEngine check_input_queue: {}", timer);
                    }
                    EngineRxMessage::Ack(rpc_id, status) => {
                        // release message meta buffer
                        self.meta_buf_pool.release(rpc_id)?;
                        let mut sent = false;
                        while !sent {
                            self.customer.enqueue_wc_with(|ptr, _count| unsafe {
                                sent = true;
                                ptr.cast::<dp::Completion>()
                                    .write(dp::Completion::Outgoing(rpc_id, status));
                                1
                            })?;
                        }
                    }
                    EngineRxMessage::RecvError(conn_id, status) => {
                        let mut sent = false;
                        while !sent {
                            self.customer.enqueue_wc_with(|ptr, _count| unsafe {
                                sent = true;
                                ptr.cast::<dp::Completion>()
                                    .write(dp::Completion::RecvError(conn_id, status));
                                1
                            })?;
                        }
                    }
                }
                Ok(Progress(1))
            }
            Err(TryRecvError::Empty) => Ok(Progress(0)),
            Err(TryRecvError::Disconnected) => Ok(Status::Disconnected),
        }
    }

    fn check_input_cmd_queue(&mut self) -> Result<Status, Error> {
        use ipc::mrpc::cmd::{Completion, CompletionKind};
        use tokio::sync::mpsc::error::TryRecvError;
        match self.cmd_rx.try_recv() {
            Ok(Completion(comp)) => {
                match comp {
                    // server new incoming connection
                    Ok(CompletionKind::NewConnectionInternal(conn_resp, fds)) => {
                        // TODO(cjr): check if this send_fd will block indefinitely.
                        self.customer.send_fd(&fds).unwrap();
                        let comp_kind = CompletionKind::NewConnection(conn_resp);
                        self.customer.send_comp(cmd::Completion(Ok(comp_kind)))?;
                        Ok(Status::Progress(1))
                    }
                    // client connection response
                    Ok(CompletionKind::ConnectInternal(conn_resp, fds)) => {
                        self.customer.send_fd(&fds).unwrap();
                        let comp_kind = CompletionKind::Connect(conn_resp);
                        self.customer.send_comp(cmd::Completion(Ok(comp_kind)))?;
                        Ok(Status::Progress(1))
                    }
                    // server bind response
                    c @ Ok(
                        CompletionKind::Bind(..)
                        | CompletionKind::NewMappedAddrs
                        | CompletionKind::UpdateProtos,
                    ) => {
                        self.customer.send_comp(cmd::Completion(c))?;
                        Ok(Status::Progress(1))
                    }
                    other => panic!("unexpected: {:?}", other),
                }
            }
            Err(TryRecvError::Empty) => Ok(Progress(0)),
            Err(TryRecvError::Disconnected) => Ok(Status::Disconnected),
        }
    }
}
