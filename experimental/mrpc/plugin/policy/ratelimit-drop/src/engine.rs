use std::collections::VecDeque;
use std::os::unix::ucred::UCred;
use std::pin::Pin;

use super::DatapathError;
use crate::config::RateLimitDropConfig;
use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use minstant::Instant;
use phoenix_api::rpc::{RpcId, TransportStatus};
use phoenix_api_policy_ratelimit_drop::control_plane;
use phoenix_common::engine::datapath::message::{EngineTxMessage, RpcMessageGeneral, RpcMessageTx};
use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::datapath::EngineRxMessage;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::log;
use phoenix_common::module::Version;
use phoenix_common::storage::{ResourceCollection, SharedStorage};
use std::num::NonZeroU32;

pub mod hello {
    include!("proto.rs");
}

fn hello_request_name_readonly(req: &hello::HelloRequest) -> String {
    let buf = &req.name as &[u8];
    String::from_utf8_lossy(buf).to_string().clone()
}

pub(crate) struct RateLimitDropEngine {
    pub(crate) node: DataPathNode,

    pub(crate) indicator: Indicator,

    // A set of func_ids to apply the rate limit.
    // TODO(cjr): maybe put this filter in a separate engine like FilterEngine/ClassiferEngine.
    // pub(crate) filter: FnvHashSet<u32>,
    // Number of tokens to add for each seconds.
    pub(crate) config: RateLimitDropConfig,
    // The most recent timestamp we add the token to the bucket.
    pub(crate) last_ts: Instant,
    // The number of available tokens in the token bucket algorithm.
    pub(crate) num_tokens: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for RateLimitDropEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "RateLimitDropEngine".to_owned()
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig(requests_per_sec, bucket_size) => {
                self.config = RateLimitDropConfig {
                    requests_per_sec,
                    bucket_size,
                };
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(RateLimitDropEngine, node);

impl Decompose for RateLimitDropEngine {
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
        collections.insert("last_ts".to_string(), Box::new(engine.last_ts));
        collections.insert("num_tokens".to_string(), Box::new(engine.num_tokens));
        (collections, engine.node)
    }
}

impl RateLimitDropEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<RateLimitDropConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let last_ts: Instant = *local
            .remove("last_ts")
            .unwrap()
            .downcast::<Instant>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let num_tokens = *local
            .remove("num_tokens")
            .unwrap()
            .downcast::<f64>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = RateLimitDropEngine {
            node,
            indicator: Default::default(),
            config,
            last_ts,
            num_tokens,
        };
        Ok(engine)
    }
}

impl RateLimitDropEngine {
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

fn current_timestamp() -> Instant {
    Instant::now()
}

#[inline]
fn materialize_nocopy(msg: &RpcMessageTx) -> &hello::HelloRequest {
    let req_ptr = msg.addr_backend as *mut hello::HelloRequest;
    let req = unsafe { req_ptr.as_ref().unwrap() };
    return req;
}

impl RateLimitDropEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use phoenix_common::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        let meta_ref = unsafe { &*msg.meta_buf_ptr.as_meta_ptr() };
                        let mut input = Vec::new();
                        input.push(msg);
                        self.num_tokens = self.num_tokens
                            + (current_timestamp() - self.last_ts).as_secs_f64()
                                * self.config.requests_per_sec as f64;
                        self.last_ts = current_timestamp();
                        log::debug!("num_tokens: {}", self.num_tokens);
                        let limit = std::cmp::min(input.len() as i64, self.num_tokens as i64);
                        self.num_tokens = self.num_tokens - limit as f64;

                        let output = input.iter().enumerate().map(|(index, req)| {
                            let rpc_message = materialize_nocopy(&req);
                            let conn_id = unsafe { &*req.meta_buf_ptr.as_meta_ptr() }.conn_id;
                            let call_id = unsafe { &*req.meta_buf_ptr.as_meta_ptr() }.call_id;
                            let rpc_id = RpcId::new(conn_id, call_id);
                            if index < limit as usize {
                                let raw_ptr: *const hello::HelloRequest = rpc_message;
                                let new_msg = RpcMessageTx {
                                    meta_buf_ptr: req.meta_buf_ptr.clone(),
                                    addr_backend: req.addr_backend,
                                };
                                RpcMessageGeneral::TxMessage(EngineTxMessage::RpcMessage(new_msg))
                            } else {
                                let error = EngineRxMessage::Ack(
                                    rpc_id,
                                    TransportStatus::Error(unsafe {
                                        NonZeroU32::new_unchecked(403)
                                    }),
                                );
                                RpcMessageGeneral::RxMessage(error)
                            }
                        });
                        for msg in output {
                            match msg {
                                RpcMessageGeneral::TxMessage(msg) => {
                                    self.tx_outputs()[0].send(msg)?;
                                }
                                RpcMessageGeneral::RxMessage(msg) => {
                                    self.rx_outputs()[0].send(msg)?;
                                }
                                _ => {}
                            }
                        }
                    }
                    m => self.tx_outputs()[0].send(m)?,
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }
        match self.rx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineRxMessage::Ack(rpc_id, status) => {
                        // todo
                        self.rx_outputs()[0].send(EngineRxMessage::Ack(rpc_id, status))?;
                    }
                    EngineRxMessage::RpcMessage(msg) => {
                        self.rx_outputs()[0].send(EngineRxMessage::RpcMessage(msg))?;
                    }
                    m => self.rx_outputs()[0].send(m)?,
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
