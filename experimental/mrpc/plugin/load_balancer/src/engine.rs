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
use slab::Slab;

use mrpc_marshal::{ExcavateContext, SgE, SgList};
use phoenix_api::buf::Range;
use phoenix_api::engine::SchedulingMode;
use phoenix_api::net::{MappedAddrStatus, WcOpcode, WcStatus};
use phoenix_api::rpc::{CallId, MessageMeta, RpcId, StatusCode, TransportStatus};
use phoenix_api::transport::tcp::dp::Completion;
use phoenix_api::{AsHandle, Handle};
use phoenix_api_load_balancer::control_plane;
use phoenix_api_mrpc::cmd::{ConnectResponse, ReadHeapRegion};
use phoenix_mrpc::unpack::UnpackFromSgE;
use phoenix_salloc::state::State as SallocState;
use transport_tcp::ops::Ops;
use transport_tcp::ApiError;

use phoenix_common::engine::datapath::message::{
    EngineRxMessage, EngineTxMessage, RpcMessageRx, RpcMessageTx,
};
use phoenix_common::engine::datapath::meta_pool::{MetaBuffer, MetaBufferPtr};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::log;
use phoenix_common::module::{ModuleCollection, Version};
use phoenix_common::resource::Error as ResourceError;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use super::get_ops;
use super::pool::BufferSlab;

use super::{ControlPathError, DatapathError};

thread_local! {
    /// To emulate a thread local storage (TLS). This should be called engine-local-storage (ELS).
    pub(crate) static ELS: RefCell<Option<&'static TlStorage>> = RefCell::new(None);
}

pub(crate) struct TlStorage {
    pub(crate) ops: Ops,
}

pub(crate) struct LoadBalancerEngine {
    pub(crate) node: DataPathNode,
    pub(crate) p2v: FnvHashMap<Handle, Handle>,
    pub(crate) v2p: FnvHashMap<Handle, Vec<Handle>>,
    pub(crate) buffer: FnvHashMap<CallId, i32>,
    pub(crate) cmd_rx_upstream:
        tokio::sync::mpsc::UnboundedReceiver<phoenix_api_mrpc::cmd::Command>,
    pub(crate) cmd_tx_upstream:
        tokio::sync::mpsc::UnboundedSender<phoenix_api_mrpc::cmd::Completion>,
    pub(crate) cmd_rx_downstream:
        tokio::sync::mpsc::UnboundedReceiver<phoenix_api_mrpc::cmd::Completion>,
    pub(crate) cmd_tx_downstream:
        tokio::sync::mpsc::UnboundedSender<phoenix_api_mrpc::cmd::Command>,
    pub(crate) _mode: SchedulingMode,
    pub(crate) indicator: Indicator,
    // pub(crate) start: std::time::Instant,
    pub(crate) rpc_ctx: Slab<RpcId>,
}

impl_vertex_for_engine!(LoadBalancerEngine, node);

impl Decompose for LoadBalancerEngine {
    #[inline]
    fn flush(&mut self) -> Result<usize> {
        // each call to `check_input_queue()` receives at most one message
        let mut work = 0;
        while !self.tx_inputs()[0].is_empty() {
            if let Progress(n) = self.check_input_queue()? {
                work += n;
            }
        }
        Ok(work)
    }

    fn decompose(
        self: Box<Self>,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode) {
        // NOTE(wyj): if command/data queue types need to be upgraded
        // then the channels must be recreated
        let engine = *self;

        let mut collections = ResourceCollection::with_capacity(14);
        log::debug!("dumping LoadBalancerEngine states...");

        let node = unsafe {
            collections.insert("mode".to_string(), Box::new(ptr::read(&engine._mode)));
            collections.insert(
                "cmd_tx_upstream".to_string(),
                Box::new(ptr::read(&engine.cmd_tx_upstream)),
            );
            collections.insert(
                "cmd_rx_upstream".to_string(),
                Box::new(ptr::read(&engine.cmd_rx_upstream)),
            );
            collections.insert(
                "cmd_tx_downstream".to_string(),
                Box::new(ptr::read(&engine.cmd_tx_downstream)),
            );
            collections.insert(
                "cmd_rx_downstream".to_string(),
                Box::new(ptr::read(&engine.cmd_rx_downstream)),
            );
            collections.insert("rpc_ctx".to_string(), Box::new(ptr::read(&engine.rpc_ctx)));
            // don't call the drop function
            ptr::read(&engine.node)
        };

        mem::forget(engine);

        (collections, node)
    }
}

impl LoadBalancerEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
        node: DataPathNode,
        _plugged: &ModuleCollection,
        _prev_version: Version,
    ) -> Result<Self> {
        log::debug!("restoring LoadBalancerEngine states...");

        let mode = *local
            .remove("mode")
            .unwrap()
            .downcast::<SchedulingMode>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let p2v = *local
            .remove("p2v")
            .unwrap()
            .downcast::<FnvHashMap<Handle, Handle>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let v2p = *local
            .remove("v2p")
            .unwrap()
            .downcast::<FnvHashMap<Handle, Vec<Handle>>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let buffer = *local
            .remove("buffer")
            .unwrap()
            .downcast::<FnvHashMap<CallId, i32>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let cmd_tx_upstream = *local
            .remove("cmd_tx_upstream")
            .unwrap()
            .downcast::<tokio::sync::mpsc::UnboundedSender<phoenix_api_mrpc::cmd::Completion>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let cmd_rx_upstream = *local
            .remove("cmd_rx_upstream")
            .unwrap()
            .downcast::<tokio::sync::mpsc::UnboundedReceiver<phoenix_api_mrpc::cmd::Command>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let cmd_tx_downstream = *local
            .remove("cmd_tx_downstream")
            .unwrap()
            .downcast::<tokio::sync::mpsc::UnboundedSender<phoenix_api_mrpc::cmd::Command>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let cmd_rx_downstream = *local
            .remove("cmd_rx_downstream")
            .unwrap()
            .downcast::<tokio::sync::mpsc::UnboundedReceiver<phoenix_api_mrpc::cmd::Completion>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let rpc_ctx = *local
            .remove("rpc_ctx")
            .unwrap()
            .downcast::<Slab<RpcId>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = LoadBalancerEngine {
            p2v,
            v2p,
            buffer,
            cmd_tx_upstream,
            cmd_rx_upstream,
            cmd_tx_downstream,
            cmd_rx_downstream,
            node,
            _mode: mode,
            indicator: Default::default(), // start: std::time::Instant::now(),
            rpc_ctx,
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

impl Engine for LoadBalancerEngine {
    fn description(self: Pin<&Self>) -> String {
        format!("LoadBalancerEngine, user",)
    }

    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
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
                        "LoadBalancer connection, Socket={:?}, local_addr={:?}, peer_addr={:?}",
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

impl Drop for LoadBalancerEngine {
    fn drop(&mut self) {
        let this = Pin::new(self);
        let desc = this.as_ref().description();
        log::warn!("{} is being dropped", desc);
    }
}

impl LoadBalancerEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            // let mut timer = utils::timer::Timer::new();

            let mut work = 0;
            // let mut nums = Vec::new();

            match self.check_input_queue()? {
                Progress(n) => {
                    work += n;
                    // nums.push(n)
                }
                Status::Disconnected => return Ok(()),
            }
            // timer.tick();

            match self.check_input_cmd_queue()? {
                Progress(n) => {
                    work += n;
                    // nums.push(n)
                }
                Status::Disconnected => return Ok(()),
            }

            match self.check_input_comp_queue()? {
                Progress(n) => {
                    work += n;
                    // nums.push(n)
                }
                Status::Disconnected => return Ok(()),
            }

            self.indicator.set_nwork(work);

            // timer.tick();
            // if work > 0 {
            //     log::info!("LoadBalancer mainloop: {:?} {}", nums, timer);
            // }
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

impl LoadBalancerEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        let conn_id = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.conn_id;

                        if conn_id.0 == u64::MAX {
                            let call_id = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.call_id;
                            self.buffer.insert(call_id, 0);
                            let rconns = self.v2p.get(&conn_id).unwrap();
                            let new_conn_id = rconns[call_id.0 as usize % rconns.len()];

                            unsafe {
                                (*msg.meta_buf_ptr.as_meta_ptr()).conn_id = new_conn_id;
                            };
                        }
                        self.tx_outputs()[0]
                            .send(EngineTxMessage::RpcMessage(msg))
                            .unwrap();
                    }
                    m => self.tx_outputs()[0].send(m).unwrap(),
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        match self.rx_inputs()[0].try_recv() {
            Ok(m) => {
                match m {
                    EngineRxMessage::Ack(rpc_id, status) => {
                        let call_id = rpc_id.1;
                        if let Some(_) = self.buffer.get(&call_id) {
                            let new_rpc_id = RpcId(Handle(u64::MAX - 1), call_id);
                            self.buffer.remove(&call_id);
                            self.rx_outputs()[0]
                                .send(EngineRxMessage::Ack(new_rpc_id, status))
                                .unwrap();
                        } else {
                            self.rx_outputs()[0]
                                .send(EngineRxMessage::Ack(rpc_id, status))
                                .unwrap();
                        }
                    }
                    _ => {
                        self.rx_outputs()[0].send(m).unwrap();
                    }
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        Ok(Progress(0))
    }

    fn check_input_cmd_queue(&mut self) -> Result<Status, ControlPathError> {
        use phoenix_api_mrpc::cmd::{Command, Completion, CompletionKind};

        use tokio::sync::mpsc::error::TryRecvError;
        match self.cmd_rx_upstream.try_recv() {
            Ok(req) => {
                match req {
                    Command::VConnect(handles) => {
                        let vid = self.gen_vconn_num();
                        log::info!("build conn mapping: {:?} -> {:?}", handles, vid);
                        self.v2p.insert(vid, handles.clone());
                        for handle in handles {
                            self.p2v.insert(handle, vid);
                        }
                        let comp = CompletionKind::VConnect(vid);
                        self.cmd_tx_upstream.send(Completion(Ok(comp)))?;
                    }
                    _ => {
                        self.cmd_tx_downstream.send(req)?;
                    }
                }
                Ok(Status::Progress(1))
            }
            Err(TryRecvError::Empty) => Ok(Status::Progress(0)),
            Err(TryRecvError::Disconnected) => Ok(Status::Disconnected),
        }
    }
    fn check_input_comp_queue(&mut self) -> Result<Status, ControlPathError> {
        use phoenix_api_mrpc::cmd::{Completion, CompletionKind};
        use tokio::sync::mpsc::error::TryRecvError;

        match self.cmd_rx_downstream.try_recv() {
            Ok(Completion(comp)) => {
                self.cmd_tx_upstream.send(Completion(comp))?;
                Ok(Status::Progress(1))
            }
            Err(TryRecvError::Empty) => Ok(Status::Progress(0)),
            Err(TryRecvError::Disconnected) => Ok(Status::Disconnected),
        }
    }

    fn gen_vconn_num(&self) -> Handle {
        return Handle(u64::MAX);
    }
}
