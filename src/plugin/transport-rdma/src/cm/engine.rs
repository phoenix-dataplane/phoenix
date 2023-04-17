use std::pin::Pin;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;

use super::super::state::State;
use super::super::ApiError;

use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{future, Decompose, Engine, EngineResult, Indicator};
use phoenix_common::envelop::ResourceDowncast;
use phoenix_common::impl_vertex_for_engine;
use phoenix_common::module::{ModuleCollection, Version};
use phoenix_common::storage::{ResourceCollection, SharedStorage};
use phoenix_common::tracing;

pub struct CmEngine {
    pub(crate) indicator: Indicator,
    pub(crate) node: DataPathNode,
    pub(crate) state: State,
}

impl CmEngine {
    pub(crate) fn new(node: DataPathNode, state: State) -> Self {
        Self {
            node,
            indicator: Default::default(),
            state,
        }
    }
}

impl Decompose for CmEngine {
    #[inline]
    fn flush(&mut self) -> Result<usize> {
        Ok(0)
    }

    fn decompose(
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
        node: DataPathNode,
        _plugged: &ModuleCollection,
        _prev_version: Version,
    ) -> Result<Self> {
        tracing::trace!("restoring RdmaTransport-CmEngine states...");
        let state = *local
            .remove("state")
            .unwrap()
            .downcast::<State>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = CmEngine {
            indicator: Default::default(),
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

impl_vertex_for_engine!(CmEngine, node);

impl Engine for CmEngine {
    fn description(self: Pin<&Self>) -> String {
        format!("RDMA CmEngine, user: {:?}", self.get_ref().state.shared.pid)
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
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

            self.indicator.set_nwork(nwork);
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
                Ok(Progress(2))
            }
            Err(e) => Err(e),
        }
    }

    fn poll_cm_event_once(&mut self) -> Result<(), ApiError> {
        // NOTE(cjr): It is fine to blocking_lock here, because we guarantee the CmEngine are
        // running on a separate Runtime.
        // TODO(cjr): the above assumption may not held anymore, fix this.
        self.state
            .shared
            .cm_manager
            .blocking_lock()
            .poll_cm_event_once(&self.state.resource().event_channel_table)
    }
}
