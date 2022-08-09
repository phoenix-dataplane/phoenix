use std::pin::Pin;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;

use koala::engine::datapath::node::DataPathNode;
use koala::engine::{future, Engine, EngineResult, Indicator, Unload};
use koala::envelop::ResourceDowncast;
use koala::impl_vertex_for_engine;
use koala::module::{ModuleCollection, Version};
use koala::storage::{ResourceCollection, SharedStorage};

use crate::state::State;
use crate::ApiError;

pub(crate) struct CmEngine {
    pub(crate) indicator: Option<Indicator>,
    pub(crate) node: DataPathNode,
    pub(crate) state: State,
}

impl CmEngine {
    pub(crate) fn new(state: State, node: DataPathNode) -> Self {
        Self {
            indicator: None,
            node,
            state,
        }
    }
}

impl_vertex_for_engine!(CmEngine, node);

impl Unload for CmEngine {
    #[inline]
    fn detach(&mut self) {}

    fn unload(
        self: Box<Self>,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode) {
        let engine = *self;
        let mut collections = ResourceCollection::with_capacity(1);
        tracing::trace!("dumping RdmaTransport-CmEngine states...");
        collections.insert("state".to_string(), Box::new(engine.state));
        (collections, engine.node)
    }
}

impl CmEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
        _plugged: &ModuleCollection,
        node: DataPathNode,
        _prev_version: Version,
    ) -> Result<Self> {
        tracing::trace!("restoring RdmaTransport-CmEngine states...");
        let state = *local
            .remove("state")
            .unwrap()
            .downcast::<State>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = CmEngine {
            indicator: None,
            node,
            state,
        };
        Ok(engine)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
}

use Status::Progress;

impl Engine for CmEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        unsafe { Box::pin(self.get_unchecked_mut().mainloop()) }
    }

    fn description(&self) -> String {
        format!("RDMA CmEngine, user: {:?}", self.state.shared.pid)
    }

    fn set_tracker(&mut self, indicator: Indicator) {
        self.indicator = Some(indicator);
    }
}

impl CmEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut nwork = 0;
            let Progress(n) = self.check_cm_event()?;
            nwork += n;
            if Arc::strong_count(&self.state.shared) == 1 {
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
