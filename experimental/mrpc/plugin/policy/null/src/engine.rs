use std::os::unix::ucred::UCred;
use std::pin::Pin;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;

use phoenix_api_policy_null::control_plane;

use phoenix_common::engine::datapath::message::EngineTxMessage;
use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::module::Version;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::NullConfig;

pub(crate) struct NullEngine {
    pub(crate) node: DataPathNode,

    pub(crate) indicator: Indicator,
    pub(crate) config: NullConfig,
    // TODO Add you own internal state here
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for NullEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "NullEngine".to_owned()
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig() => {
                self.config = NullConfig {};
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(NullEngine, node);

impl Decompose for NullEngine {
    fn flush(&mut self) -> Result<usize> {
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
        let engine = *self;

        let mut collections = ResourceCollection::with_capacity(4);
        collections.insert("config".to_string(), Box::new(engine.config));
        (collections, engine.node)
        // TODO if you have internal state that need to be stored
        // add it to collections
    }
}

impl NullEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<NullConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        // TODO if you have stored internal state
        // load it from collection here
        let engine = NullEngine {
            node,
            indicator: Default::default(),
            config,
        };
        Ok(engine)
    }
}

impl NullEngine {
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

impl NullEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;
        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        // TODO Add logic for tx input  here
                        self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
                    }
                    m => self.tx_outputs()[0].send(m)?,
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        match self.rx_inputs()[0].try_recv() {
            Ok(m) => {
                // TODO Add logic for rx input here
                // use match statement like tx
                self.rx_outputs()[0].send(m)?;
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        Ok(Progress(0))
    }
}
