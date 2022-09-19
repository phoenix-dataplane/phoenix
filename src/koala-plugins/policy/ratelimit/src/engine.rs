use std::collections::VecDeque;
use std::os::unix::ucred::UCred;
use std::pin::Pin;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use minstant::Instant;

use ipc::ratelimit::control_plane;

use koala::engine::datapath::message::{EngineTxMessage, RpcMessageTx};
use koala::engine::datapath::node::DataPathNode;
use koala::engine::{future, Decompose, Engine, EngineResult, Indicator, Vertex};
use koala::envelop::ResourceDowncast;
use koala::impl_vertex_for_engine;
use koala::module::Version;
use koala::storage::{ResourceCollection, SharedStorage};

use super::DatapathError;
use crate::config::RateLimitConfig;

pub(crate) struct RateLimitEngine {
    pub(crate) node: DataPathNode,

    pub(crate) indicator: Indicator,

    // A set of func_ids to apply the rate limit.
    // TODO(cjr): maybe put this filter in a separate engine like FilterEngine/ClassiferEngine.
    // pub(crate) filter: FnvHashSet<u32>,
    // Number of tokens to add for each seconds.
    pub(crate) config: RateLimitConfig,
    // The most recent timestamp we add the token to the bucket.
    pub(crate) last_ts: Instant,
    // The number of available tokens in the token bucket algorithm.
    pub(crate) num_tokens: f64,
    // The queue to buffer the requests that cannot be sent immediately.
    pub(crate) queue: VecDeque<RpcMessageTx>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for RateLimitEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    fn description(self: Pin<&Self>) -> String {
        format!("RateLimitEngine")
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn handle_request(&mut self, request: Vec<u8>, _cred: UCred) -> Result<()> {
        let request: control_plane::Request = bincode::deserialize(&request[..])?;

        match request {
            control_plane::Request::NewConfig(requests_per_sec, bucket_size) => {
                self.config = RateLimitConfig {
                    requests_per_sec,
                    bucket_size,
                };
            }
        }
        Ok(())
    }
}

impl_vertex_for_engine!(RateLimitEngine, node);

impl Decompose for RateLimitEngine {
    fn flush(&mut self) -> Result<()> {
        // while !self.tx_inputs()[0].is_empty() {
        //     self.check_input_queue()?;
        // }
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
        collections.insert("num_tokens".to_string(), Box::new(engine.num_tokens));
        collections.insert("queue".to_string(), Box::new(engine.queue));
        (collections, engine.node)
    }
}

impl RateLimitEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        let config = *local
            .remove("config")
            .unwrap()
            .downcast::<RateLimitConfig>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let last_ts = *local
            .remove("last_ts")
            .unwrap()
            .downcast::<Instant>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let num_tokens = *local
            .remove("num_tokens")
            .unwrap()
            .downcast::<f64>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let queue = *local
            .remove("queue")
            .unwrap()
            .downcast::<VecDeque<RpcMessageTx>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = RateLimitEngine {
            node,
            indicator: Default::default(),
            config,
            last_ts,
            num_tokens,
            queue,
        };
        Ok(engine)
    }
}

impl RateLimitEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            self.add_tokens();

            let mut work = 0;
            // check input queue, ~100ns
            loop {
                match self.check_input_queue()? {
                    Progress(0) => break,
                    Progress(n) => work += n,
                    Status::Disconnected => return Ok(()),
                }
            }

            self.add_tokens();

            self.leak_bucket()?;

            // If there's pending receives, there will always be future work to do.
            self.indicator.set_nwork(work);

            future::yield_now().await;
        }
    }
}

impl RateLimitEngine {
    #[inline]
    fn add_tokens(&mut self) {
        let now = Instant::now();
        let dura = now - self.last_ts;
        let config = &self.config;
        let requests_per_sec = config.requests_per_sec;
        let bucket_size = config.bucket_size as usize;
        if dura.as_secs_f64() * requests_per_sec as f64 >= 1.0 {
            self.num_tokens += dura.as_secs_f64() * requests_per_sec as f64;
            if self.num_tokens > bucket_size as f64 {
                self.num_tokens = bucket_size as f64;
            }
            self.last_ts = now;
        }
    }

    fn leak_bucket(&mut self) -> Result<(), DatapathError> {
        while self.num_tokens > 0.1 && !self.queue.is_empty() {
            let msg = self.queue.pop_front().unwrap();
            self.num_tokens -= 1.0;
            self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
        }

        Ok(())
    }

    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use koala::engine::datapath::TryRecvError;

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        if !self.queue.is_empty() || self.num_tokens < 0.1 {
                            self.queue.push_back(msg);
                        } else {
                            self.num_tokens -= 1.0;
                            self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
                        }
                    }
                    // XXX TODO(cjr): it is best not to reorder the message
                    m @ _ => self.tx_outputs()[0].send(m)?,
                }
                return Ok(Progress(1));
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        }

        Ok(Progress(0))
    }
}
