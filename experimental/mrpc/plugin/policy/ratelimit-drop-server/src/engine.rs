use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use minstant::Instant;
use phoenix_api::rpc::{RpcId, StatusCode, TransportStatus};
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::num::NonZeroU32;
use std::os::unix::ucred::UCred;
use std::pin::Pin;
use std::ptr::Unique;

use phoenix_api_policy_ratelimit_drop_server::control_plane;
use phoenix_common::engine::datapath::message::{
    EngineRxMessage, EngineTxMessage, RpcMessageGeneral,
};
use phoenix_common::engine::datapath::meta_pool::MetaBufferPool;

use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::log;
use phoenix_common::module::Version;

use phoenix_common::engine::datapath::{RpcMessageRx, RpcMessageTx};
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::{create_log_file, RateLimitDropServerConfig};

use chrono::prelude::*;
use itertools::iproduct;
use rand::Rng;

pub mod hello {
    include!("proto.rs");
}

fn hello_request_name_readonly(req: &hello::HelloRequest) -> String {
    let buf = &req.name as &[u8];
    String::from_utf8_lossy(buf).to_string().clone()
}

pub(crate) struct RateLimitDropServerEngine {
    pub(crate) node: DataPathNode,

    pub(crate) indicator: Indicator,

    // A set of func_ids to apply the rate limit.
    // TODO(cjr): maybe put this filter in a separate engine like FilterEngine/ClassiferEngine.
    // pub(crate) filter: FnvHashSet<u32>,
    // Number of tokens to add for each seconds.
    pub(crate) config: RateLimitDropServerConfig,
    pub(crate) meta_buf_pool: MetaBufferPool,

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

impl Engine for RateLimitDropServerEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        "RateLimitDropServerEngine".to_owned()
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig(requests_per_sec, bucket_size) => {
                self.config = RateLimitDropServerConfig {
                    requests_per_sec,
                    bucket_size,
                };
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(RateLimitDropServerEngine, node);

impl Decompose for RateLimitDropServerEngine {
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
        let mut collections = ResourceCollection::with_capacity(4);
        collections.insert("config".to_string(), Box::new(engine.config));
        collections.insert("meta_buf_pool".to_string(), Box::new(engine.meta_buf_pool));
        collections.insert("last_ts".to_string(), Box::new(engine.last_ts));
        collections.insert("num_tokens".to_string(), Box::new(engine.num_tokens));
        (collections, engine.node)
    }
}

impl RateLimitDropServerEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<RateLimitDropServerConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let meta_buf_pool = *local
            .remove("meta_buf_pool")
            .unwrap()
            .downcast::<MetaBufferPool>()
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

        let engine = RateLimitDropServerEngine {
            node,
            indicator: Default::default(),
            config,
            last_ts,
            num_tokens,
            meta_buf_pool,
        };
        Ok(engine)
    }
}

impl RateLimitDropServerEngine {
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

/// Copy the RPC request to a private heap and returns the request.
#[inline]
fn materialize_rx(msg: &RpcMessageRx) -> Box<hello::HelloRequest> {
    let req_ptr = Unique::new(msg.addr_backend as *mut hello::HelloRequest).unwrap();
    let req = unsafe { req_ptr.as_ref() };
    // returns a private_req
    Box::new(req.clone())
}

impl RateLimitDropServerEngine {
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
                    EngineRxMessage::Ack(rpc_id, status) => {
                        if let Ok(()) = self.meta_buf_pool.release(rpc_id) {
                        } else {
                            self.rx_outputs()[0].send(m)?;
                        }
                    }
                    EngineRxMessage::RpcMessage(msg) => {
                        self.num_tokens = self.num_tokens
                            + (current_timestamp() - self.last_ts).as_secs_f64()
                                * self.config.requests_per_sec as f64;
                        self.last_ts = current_timestamp();
                        if self.num_tokens < 1.0 {
                            // We need to copy meta, add it to meta_buf_pool, and send it as the tx msg
                            // Is there better way to do this and avoid unsafe?
                            let mut meta = unsafe { msg.meta.as_ref().clone() };
                            meta.status_code = StatusCode::AccessDenied;
                            let mut meta_ptr = self
                                .meta_buf_pool
                                .obtain(RpcId(meta.conn_id, meta.call_id))
                                .expect("meta_buf_pool is full");
                            unsafe {
                                meta_ptr.as_meta_ptr().write(meta);
                                meta_ptr.0.as_mut().num_sge = 0;
                                meta_ptr.0.as_mut().value_len = 0;
                            }
                            let rpc_msg = RpcMessageTx {
                                meta_buf_ptr: meta_ptr,
                                addr_backend: 0,
                            };
                            let new_msg = EngineTxMessage::RpcMessage(rpc_msg);
                            self.tx_outputs()[0]
                                .send(new_msg)
                                .expect("send new message error");
                            let msg_call_ids =
                                [meta.call_id, meta.call_id, meta.call_id, meta.call_id];
                            self.tx_outputs()[0].send(EngineTxMessage::ReclaimRecvBuf(
                                meta.conn_id,
                                msg_call_ids,
                            ))?;
                        } else {
                            self.num_tokens = self.num_tokens - 1.0;
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
            Err(TryRecvError::Disconnected) => {
                return Ok(Status::Disconnected);
            }
        }
        Ok(Progress(0))
    }
}
