use std::collections::VecDeque;
use std::pin::Pin;
use std::time::Duration;

use futures::future::BoxFuture;
use minstant::Instant;

use super::DatapathError;
use crate::engine::graph::{EngineTxMessage, RpcMessageTx};
use crate::engine::{future, Engine, EngineResult, Indicator, Vertex};
use crate::node::Node;

pub(crate) struct RateLimitEngine {
    pub(crate) node: Node,

    pub(crate) indicator: Indicator,

    // A set of func_ids to apply the rate limit.
    // TODO(cjr): maybe put this filter in a separate engine like FilterEngine/ClassiferEngine.
    // pub(crate) filter: FnvHashSet<u32>,
    // Number of tokens to add for each seconds.
    pub(crate) requests_per_sec: u64,
    // The most recent timestamp we add the token to the bucket.
    pub(crate) last_ts: Instant,
    // The number of available tokens in the token bucket algorithm.
    pub(crate) num_tokens: usize,
    // The queue to buffer the requests that cannot be sent immediately.
    pub(crate) queue: VecDeque<RpcMessageTx>,
}

crate::unimplemented_ungradable!(RateLimitEngine);
crate::impl_vertex_for_engine!(RateLimitEngine, node);

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
}

impl RateLimitEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut work = 0;
            // check input queue, ~100ns
            match self.check_input_queue()? {
                Progress(n) => work += n,
                Status::Disconnected => return Ok(()),
            }

            self.add_tokens();

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
        if dura * self.requests_per_sec as u32 >= Duration::from_secs(1) {
            self.num_tokens +=
                (dura.as_nanos() as f64 * self.requests_per_sec as f64 / 1e9) as usize;
            self.last_ts = now;
        }
    }

    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use crate::engine::graph::TryRecvError;

        while self.num_tokens > 0 && !self.queue.is_empty() {
            let msg = self.queue.pop_front().unwrap();
            self.num_tokens -= 1;
            self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
        }

        match self.tx_inputs()[0].try_recv() {
            Ok(msg) => {
                match msg {
                    EngineTxMessage::RpcMessage(msg) => {
                        if !self.queue.is_empty() || self.num_tokens == 0 {
                            self.queue.push_back(msg);
                        } else {
                            self.num_tokens -= 1;
                            self.tx_outputs()[0].send(EngineTxMessage::RpcMessage(msg))?;
                        }
                    }
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
