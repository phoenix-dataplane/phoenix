use std::collections::VecDeque;
use std::os::unix::ucred::UCred;
use std::pin::Pin;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use minstant::Instant;

use phoenix_api_policy_delay::control_plane;

use phoenix_common::engine::datapath::message::{EngineTxMessage, RpcMessageTx};
use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::module::Version;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::DelayConfig;

pub(crate) struct DelayRpcInfo {
    pub(crate) msg: RpcMessageTx,
    pub(crate) timestamp: Instant,
}

pub(crate) struct DelayEngine {
    pub(crate) node: DataPathNode,
    pub(crate) indicator: Indicator,
    pub(crate) config: DelayConfig,
    pub(crate) var_probability: f32,
    // The queue to buffer delayed requests.
    pub(crate) queue: VecDeque<DelayRpcInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for DelayEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "DelayEngine".to_owned()
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig() => {
                self.config = DelayConfig {};
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(DelayEngine, node);

impl Decompose for DelayEngine {
    fn flush(&mut self) -> Result<usize> {
        let mut work = 0;
        while !self.tx_inputs()[0].is_empty() {
            if let Progress(n) = self.check_input_queue()? {
                work += n;
            }
        }
        while !self.queue.is_empty() {
            let DelayRpcInfo { msg, .. } = self.queue.pop_front().unwrap();
            self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
            work += 1;
        }
        Ok(work)
    }

    fn decompose(
        self: Box<Self>,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode) {
        let engine = *self;
        let mut collections = ResourceCollection::with_capacity(4);
        collections.insert("config".to_string(), Box::new(engine.config));
        collections.insert(
            "var_probability".to_string(),
            Box::new(engine.var_probability),
        );
        collections.insert("queue".to_string(), Box::new(engine.queue));
        (collections, engine.node)
    }
}

impl DelayEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<DelayConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let var_probability = 0.2;
        let queue = *local
            .remove("queue")
            .unwrap()
            .downcast::<VecDeque<DelayRpcInfo>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = DelayEngine {
            node,
            indicator: Default::default(),
            config,
            var_probability,
            queue,
        };
        Ok(engine)
    }
}

impl DelayEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut work = 0;
            loop {
                match self.check_input_queue()? {
                    Progress(0) => break,
                    Progress(n) => work += n,
                    Status::Disconnected => return Ok(()),
                }
            }
            self.check_delay_buffer()?;
            self.indicator.set_nwork(work);
            future::yield_now().await;
        }
    }
}

impl DelayEngine {
    fn check_delay_buffer(&mut self) -> Result<(), DatapathError> {
        while !self.queue.is_empty() {
            let oldest_msg = self.queue.pop_front().unwrap();
            if oldest_msg.timestamp.elapsed().as_millis() > 100 {
                self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(oldest_msg.msg))?;
            } else {
                self.queue.push_front(oldest_msg);
                break;
            }
        }
        Ok(())
    }

    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        if rand::random::<f32>() < self.var_probability {
                            let delay_msg = DelayRpcInfo {
                                msg: msg,
                                timestamp: Instant::now(),
                            };
                            self.queue.push_back(delay_msg);
                        } else {
                            self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
                        }
                    }
                    m => self.tx_outputs()[0].send(m)?,
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                return Ok(Status::Disconnected);
            }
        }

        Ok(Progress(0))
    }
}
