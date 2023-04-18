use std::collections::VecDeque;
use std::os::unix::ucred::UCred;
use std::pin::Pin;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use minstant::Instant;
use rand::prelude::*;

use phoenix_api_policy_breakwater::control_plane;

use phoenix_common::engine::datapath::message::{EngineRxMessage, EngineTxMessage, RpcMessageTx};
use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::log;
use phoenix_api::rpc::RpcId;
use phoenix_common::module::Version;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::BreakWaterConfig;

/// BreakWaterEngine is the engine for the BreakWater policy.
pub(crate) struct BreakWaterEngine {
    pub(crate) node: DataPathNode,

    pub(crate) indicator: Indicator,

    // A set of func_ids to apply the rate limit.
    // TODO(cjr): maybe put this filter in a separate engine like FilterEngine/ClassiferEngine.
    // pub(crate) filter: FnvHashSet<u32>,
    // Number of tokens to add for each seconds.
    pub(crate) config: BreakWaterConfig,
    // The most recent timestamp we add the token to the bucket.
    pub(crate) last_ts: Instant,
    // The number of available tokens in the token bucket algorithm.
    // pub(crate) num_tokens: f64,
    // The queue to buffer the requests that cannot be sent immediately.
    pub(crate) queue: VecDeque<RpcMessageTx>,
}

// Status is the status of the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

// Check the input queue and send the requests that can be sent.
impl Engine for BreakWaterEngine {

    // Activate the engine with a mainloop.
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    // A description of the engine.
    fn description(self: Pin<&Self>) -> String {
        "BreakWaterEngine".to_owned()
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    // Handle only requests of type control_plane::Request::NewConfig.
    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        // Deserialize the request.
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig() => {
                self.config = BreakWaterConfig {
                };
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(BreakWaterEngine, node);

impl Decompose for BreakWaterEngine {
    fn flush(&mut self) -> Result<()> {
        while !self.tx_inputs()[0].is_empty() {
            self.check_input_queue()?;
        }
        while !self.queue.is_empty() {
            let msg = self.queue.pop_front().unwrap();
            self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
        }
        Ok(())
    }

    fn decompose(
        self: Box<Self>,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode) {
        let engine = *self;

        let mut collections = ResourceCollection::with_capacity(4);
        collections.insert("config".to_string(), Box::new(engine.config));
        collections.insert("last_ts".to_string(), Box::new(engine.last_ts));
        //collections.insert("num_tokens".to_string(), Box::new(engine.num_tokens));
        collections.insert("queue".to_string(), Box::new(engine.queue));
        (collections, engine.node)
    }
}

impl BreakWaterEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<BreakWaterConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let last_ts = *local
            .remove("last_ts")
            .unwrap()
            .downcast::<Instant>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let queue = *local
            .remove("queue")
            .unwrap()
            .downcast::<VecDeque<RpcMessageTx>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = BreakWaterEngine {
            node,
            indicator: Default::default(),
            config,
            last_ts,
            queue,
        };
        Ok(engine)
    }
}

impl BreakWaterEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut work = 0;
            // check input queue, ~100ns
            loop {
                match self.check_input_queue()? {
                    Progress(0) => break,
                    Progress(n) => work += n,
                    Status::Disconnected => return Ok(()),
                }
            }

            // If there's pending receives, there will always be future work to do.
            self.indicator.set_nwork(work);

            future::yield_now().await;
        }
    }
}

impl BreakWaterEngine {
    #[inline]
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        let meta_ref = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() };
                        log::info!("Got a TX message: {:?}", meta_ref);
                        
                        let conn_id = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.conn_id;
                        let call_id = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.call_id;
                        let rpc_id = RpcId::new(conn_id, call_id);

                        let mut rng = rand::thread_rng();

                        let mut request_timestamp = Instant::now();
                        let mut request_credit = rng.gen_range(1..1000);

                        log::info!("RPC ID: {:?}", rpc_id);

                        self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
                    }

                    m => self.tx_outputs()[0].send(m)?,
                }
                //self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        match self.rx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineRxMessage::RpcMessage(msg) => {
                        //let meta_ref = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() };
                        log::info!("Got an RX message");

                        //let conn_id = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.conn_id;
                        //let call_id = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() }.call_id;
                        //let rpc_id = RpcId::new(conn_id, call_id);

                        //log::info!("RPC ID: {:?}", rpc_id),

                        log::info!("Breakwater Meta: Timestamp  {:?}", msg.request_timestamp);
                        log::info!("Breakwater Meta: Credit  {:?}", msg.request_credit);
                                                
                        self.rx_outputs()[0].send(EngineRxMessage::RpcMessage(msg))?;
                    }

                    m => self.rx_outputs()[0].send(m)?,
                }
                //self.rx_outputs()[0].send(EngineRxMessage::RpcMessage(msg))?;
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        Ok(Progress(0))
    }
}
