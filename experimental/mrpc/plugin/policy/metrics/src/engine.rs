//! This engine can only be placed at the sender side for now.
use std::num::NonZeroU32;
use std::os::unix::ucred::UCred;
use std::pin::Pin;
use std::ptr::Unique;

use anyhow::{anyhow, Result};
use fnv::FnvHashMap as HashMap;
use futures::future::BoxFuture;

use phoenix_api::rpc::{RpcId, StatusCode, TransportStatus};
use phoenix_api_policy_metrics::control_plane;

use phoenix_common::engine::datapath::message::{EngineRxMessage, EngineTxMessage, RpcMessageTx};
use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::log;
use phoenix_common::module::Version;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::MetricsConfig;

pub(crate) struct MetricsEngine {
    pub(crate) node: DataPathNode,

    pub(crate) indicator: Indicator,

    pub(crate) num_succ: u32,
    pub(crate) num_rej: u32,

    pub(crate) config: MetricsConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for MetricsEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "MetricsEngine".to_owned()
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig() => {
                // Update config
                self.config = MetricsConfig {};
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(MetricsEngine, node);

impl Decompose for MetricsEngine {
    fn flush(&mut self) -> Result<usize> {
        let mut work = 0;
        while !self.tx_inputs()[0].is_empty() || !self.rx_inputs()[0].is_empty() {
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
        let engine = *self;

        let mut collections = ResourceCollection::with_capacity(2);
        collections.insert("config".to_string(), Box::new(engine.config));
        collections.insert("num_rej".to_string(), Box::new(engine.num_rej));
        collections.insert("num_succ".to_string(), Box::new(engine.num_succ));
        (collections, engine.node)
    }
}

impl MetricsEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<MetricsConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let num_succ = *local
            .remove("num_succ")
            .unwrap()
            .downcast::<u32>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let num_rej = *local
            .remove("num_rej")
            .unwrap()
            .downcast::<u32>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let engine = MetricsEngine {
            node,
            indicator: Default::default(),
            num_succ,
            num_rej,
            config,
        };
        Ok(engine)
    }
}

impl MetricsEngine {
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

impl MetricsEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                self.tx_outputs()[0].send(msg)?;
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        // forward all rx msgs
        match self.rx_inputs()[0].try_recv() {
            Ok(m) => {
                match m {
                    EngineRxMessage::RpcMessage(msg) => {
                        let meta = unsafe { &*msg.meta.as_ptr() };
                        if meta.status_code == StatusCode::Success {
                            self.num_succ += 1;
                        } else {
                            self.num_rej += 1;
                        }
                        self.rx_outputs()[0].send(EngineRxMessage::RpcMessage(msg))?;
                    }
                    m => {
                        self.rx_outputs()[0].send(m)?;
                    }
                };
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        Ok(Progress(0))
    }
}
