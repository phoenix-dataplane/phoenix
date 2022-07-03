use std::future::Future;

use super::super::state::State;
use super::super::ApiError;
use crate::engine::{future, Engine, EngineResult, Indicator};
use crate::node::Node;

pub struct CmEngine {
    pub(crate) node: Node,
    pub(crate) indicator: Option<Indicator>,
    pub(crate) state: State,
}

impl CmEngine {
    pub(crate) fn new(node: Node, state: State) -> Self {
        Self {
            node,
            indicator: None,
            state,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
}

use Status::Progress;

crate::unimplemented_ungradable!(CmEngine);
crate::impl_vertex_for_engine!(CmEngine, node);

impl Engine for CmEngine {
    type Future = impl Future<Output = EngineResult>;

    fn description(&self) -> String {
        format!("RDMA CmEngine, user: {:?}", self.state.shared.pid)
    }

    fn set_tracker(&mut self, indicator: Indicator) {
        self.indicator = Some(indicator);
    }

    fn entry(mut self) -> Self::Future {
        Box::pin(async move { self.mainloop().await })
    }
}

impl CmEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut nwork = 0;
            let Progress(n) = self.check_cm_event()?;
            nwork += n;
            if self.state.alive_engines() == 1 {
                // cm_engine is the last active engine
                return Ok(());
            }

            self.indicator.as_ref().unwrap().set_nwork(nwork);
            future::yield_now().await;
        }
    }
}

impl CmEngine {
    fn check_cm_event(&mut self) -> Result<Status, ApiError> {
        match self.poll_cm_event_once() {
            Ok(()) => Ok(Progress(1)),
            Err(ApiError::NoCmEvent) => Ok(Progress(0)),
            Err(e @ (ApiError::RdmaCm(_) | ApiError::Mio(_))) => {
                self.state
                    .shared
                    .cm_manager
                    .blocking_lock()
                    .err_buffer
                    .push_back(e);
                return Ok(Progress(1));
            }
            Err(e) => return Err(e),
        }
    }

    fn poll_cm_event_once(&mut self) -> Result<(), ApiError> {
        // NOTE(cjr): It is fine to blocking_lock here, because we guarantee the CmEngine are
        // running on a separate Runtime.
        self.state
            .shared
            .cm_manager
            .blocking_lock()
            .poll_cm_event_once(&self.state.resource().event_channel_table)
    }
}
