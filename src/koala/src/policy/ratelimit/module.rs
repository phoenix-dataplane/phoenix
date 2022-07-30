use std::collections::VecDeque;
use std::future::Future;

use fnv::FnvHashMap;

use interface::engine::SchedulingMode;
use interface::rpc::{MessageMeta, RpcId, RpcMsgType, TransportStatus};
use interface::{AsHandle, Handle};

use super::{ControlPathError, DatapathError};
use crate::engine::graph::{EngineTxMessage, RpcMessageRx, RpcMessageTx};
use crate::engine::{
    future, Engine, EngineLocalStorage, EngineResult, EngineRxMessage, Indicator, Vertex,
};
use crate::node::Node;

pub(crate) struct RateLimitEngine {
    pub(crate) node: Node,

    pub(crate) _mode: SchedulingMode,
    pub(crate) indicator: Option<Indicator>,
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
    type Future = impl Future<Output = EngineResult>;

    fn description(&self) -> String {
        format!("RateLimitEngine")
    }

    fn set_tracker(&mut self, indicator: Indicator) {
        self.indicator = Some(indicator);
    }

    fn entry(mut self) -> Self::Future {
        Box::pin(async move { self.mainloop().await })
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

            // If there's pending receives, there will always be future work to do.
            self.indicator
                .as_ref()
                .unwrap()
                .set_nwork(work);

            future::yield_now().await;
        }
    }
}

impl RateLimitEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        use crate::engine::graph::TryRecvError;

        // match self.tx_inputs()[0].try_recv() {
        //     Ok(msg) => {}
        //     Err(TryRecvError::Empty) => {}
        //     Err(TryRecvError::Disconnected) => return Ok(Status::Disconnected),
        // }

        Ok(Progress(0))
    }
}
