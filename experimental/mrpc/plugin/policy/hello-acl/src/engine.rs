//! This engine can only be placed at the sender side for now.
use std::num::NonZeroU32;
use std::os::unix::ucred::UCred;
use std::pin::Pin;
use std::ptr::Unique;
use std::sync::mpsc::RecvError;

use anyhow::{anyhow, Result};
use fnv::FnvHashMap as HashMap;
use futures::future::BoxFuture;

use phoenix_api::rpc::{RpcId, TransportStatus};
use phoenix_api_policy_hello_acl::control_plane;

use phoenix_common::engine::datapath::message::{
    EngineRxMessage, EngineTxMessage, RpcMessageRx, RpcMessageTx,
};
use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::log;
use phoenix_common::module::Version;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::HelloAclConfig;

pub mod hello {
    // The string specified here must match the proto package name
    include!("rpc_hello.rs");
}

pub(crate) struct HelloAclEngine {
    pub(crate) node: DataPathNode,

    pub(crate) indicator: Indicator,

    pub(crate) outstanding_req_pool: HashMap<RpcId, Box<hello::HelloRequest>>,

    // A set of func_ids to apply the rate limit.
    // TODO(cjr): maybe put this filter in a separate engine like FilterEngine/ClassiferEngine.
    // pub(crate) filter: FnvHashSet<u32>,
    // Number of tokens to add for each seconds.
    pub(crate) config: HelloAclConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for HelloAclEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "HelloAclEngine".to_owned()
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig => {
                // Update config
                self.config = HelloAclConfig {};
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(HelloAclEngine, node);

impl Decompose for HelloAclEngine {
    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    fn decompose(
        self: Box<Self>,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode) {
        let engine = *self;

        let mut collections = ResourceCollection::with_capacity(2);
        collections.insert(
            "outstanding_req_pool".to_string(),
            Box::new(engine.outstanding_req_pool),
        );
        collections.insert("config".to_string(), Box::new(engine.config));
        (collections, engine.node)
    }
}

impl HelloAclEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let outstanding_req_pool = *local
            .remove("outstanding_req_pool")
            .unwrap()
            .downcast::<HashMap<RpcId, Box<hello::HelloRequest>>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<HelloAclConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = HelloAclEngine {
            node,
            indicator: Default::default(),
            outstanding_req_pool,
            config,
        };
        Ok(engine)
    }
}

impl HelloAclEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut work = 0;
            // check input queue, ~100ns
            loop {
                match self.receiver_check_input_queue()? {
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

/// Copy the RPC request to a private heap and returns the request.
#[inline]
fn materialize(msg: &RpcMessageTx) -> Box<hello::HelloRequest> {
    let req_ptr = Unique::new(msg.addr_backend as *mut hello::HelloRequest).unwrap();
    let req = unsafe { req_ptr.as_ref() };
    // returns a private_req
    Box::new(req.clone())
}

/// Copy the RPC request to a private heap and returns the request.
#[inline]
fn materialize_rx(msg: &RpcMessageRx) -> Box<hello::HelloRequest> {
    let req_ptr = Unique::new(msg.addr_backend as *mut hello::HelloRequest).unwrap();
    let req = unsafe { req_ptr.as_ref() };
    // returns a private_req
    Box::new(req.clone())
}

#[inline]
fn should_block(req: &hello::HelloRequest) -> bool {
    let buf = &req.name as &[u8];
    // todo this is O(n)
    let name = String::from_utf8_lossy(buf);
    log::info!("raw: {:?}, req.name: {:?}", buf, name);
    name == "mRPC"
}

impl HelloAclEngine {
    fn receiver_check_input_queue(&mut self) -> Result<Status, DatapathError> {
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
                    EngineRxMessage::Ack(rpc_id, _status) => {
                        self.outstanding_req_pool.remove(&rpc_id);
                        self.rx_outputs()[0].send(m)?;
                    }
                    EngineRxMessage::RpcMessage(msg) => {
                        let req_ptr =
                            Unique::new(msg.addr_backend as *mut hello::HelloRequest).unwrap();
                        let req = unsafe { req_ptr.as_ref() };
                        if should_block(req) {
                            let rpc_id = unsafe {
                                RpcId::new(msg.meta.as_ref().conn_id, msg.meta.as_ref().call_id)
                            };
                            // let error = EngineTxMessage::RpcMessage(
                            //     rpc_id,
                            //     TransportStatus::Error(unsafe { NonZeroU32::new_unchecked(403) }),
                            // );

                            // self.tx_outputs()[0].send(error).unwrap_or_else(|e| {
                            //     log::warn!("error when bubbling up the error, send failed e: {}", e)
                            // });
                            log::warn!("acl denied on rx");
                        } else {
                            self.rx_outputs()[0].send(EngineRxMessage::RpcMessage(msg))?;
                        }
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
    fn sender_check_input_queue(&mut self) -> Result<Status, DatapathError> {
        Ok(Progress(0))
    }
}
