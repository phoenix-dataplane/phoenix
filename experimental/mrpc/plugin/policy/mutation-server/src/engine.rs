//! This engine can only be placed at the sender side for now.
use std::num::NonZeroU32;
use std::os::unix::ucred::UCred;
use std::pin::Pin;
use std::ptr::Unique;

use anyhow::{anyhow, Result};
use fnv::FnvHashMap as HashMap;
use futures::future::BoxFuture;

use phoenix_api::rpc::{RpcId, TransportStatus};
use phoenix_api_policy_mutation_server::control_plane;

use phoenix_common::engine::datapath::message::{EngineRxMessage, EngineTxMessage, RpcMessageTx};
use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::datapath::RpcMessageRx;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::log;
use phoenix_common::module::Version;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::MutationServerConfig;

pub(crate) struct MutationServerEngine {
    pub(crate) node: DataPathNode,

    pub(crate) indicator: Indicator,

    pub(crate) config: MutationServerConfig,

    pub(crate) target: String,
}

pub mod hello {
    // The string specified here must match the proto package name
    include!("rpc_hello.rs");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for MutationServerEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "MutationServerEngine".to_owned()
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
                self.config = MutationServerConfig {};
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(MutationServerEngine, node);

impl Decompose for MutationServerEngine {
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
        collections.insert("target".to_string(), Box::new(engine.target));
        collections.insert("config".to_string(), Box::new(engine.config));
        (collections, engine.node)
    }
}

impl MutationServerEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<MutationServerConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let target = *local
            .remove("target")
            .unwrap()
            .downcast::<String>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let engine = MutationServerEngine {
            node,
            indicator: Default::default(),
            target,
            config,
        };
        Ok(engine)
    }
}

impl MutationServerEngine {
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

#[inline]
fn materialize_nocopy(msg: &RpcMessageRx) -> &mut hello::HelloRequest {
    let req_ptr = msg.addr_backend as *mut hello::HelloRequest;
    let req = unsafe { req_ptr.as_mut().unwrap() };
    return req;
}

impl MutationServerEngine {
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

        match self.rx_inputs()[0].try_recv() {
            Ok(m) => {
                match m {
                    EngineRxMessage::Ack(rpc_id, _status) => {
                        self.rx_outputs()[0].send(m)?;
                    }
                    EngineRxMessage::RpcMessage(msg) => {
                        let mut req = materialize_nocopy(&msg);
                        for i in 0..req.name.len() {
                            req.name[i] = 'a' as u8;
                        }
                        self.rx_outputs()[0].send(EngineRxMessage::RpcMessage(msg))?;
                    }
                    EngineRxMessage::RecvError(_, _) => {
                        self.rx_outputs()[0].send(m)?;
                    }
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        Ok(Progress(0))
    }
}
