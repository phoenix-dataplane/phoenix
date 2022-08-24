use std::collections::VecDeque;
use std::pin::Pin;

use anyhow::{anyhow, Result};
use futures::future::BoxFuture;

use koala::engine::datapath::DataPathNode;
use koala::engine::{future, Decompose, Engine, EngineResult, Indicator};
use koala::envelop::ResourceDowncast;
use koala::impl_vertex_for_engine;
use koala::module::{ModuleCollection, Version};
use koala::storage::{ResourceCollection, SharedStorage};
use koala::tracing;

use transport_rdma::ops::Ops;

use super::super::state::State;
use super::super::ControlPathError;
use crate::ulib;

pub struct AcceptorEngine {
    pub(crate) indicator: Option<Indicator>,
    pub(crate) node: DataPathNode,
    pub(crate) state: State,
    pub(crate) ops: Box<Ops>,
}

impl_vertex_for_engine!(AcceptorEngine, node);

impl AcceptorEngine {
    pub(crate) fn new(state: State, ops: Box<Ops>, node: DataPathNode) -> Self {
        Self {
            indicator: None,
            node,
            state,
            ops,
        }
    }
}

impl Decompose for AcceptorEngine {
    #[inline]
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

        tracing::trace!("dumping RpcAdapter-AcceptorEngine states...");
        collections.insert("state".to_string(), Box::new(engine.state));
        collections.insert("ops".to_string(), Box::new(engine.ops));
        (collections, engine.node)
    }
}

impl AcceptorEngine {
    pub(crate) fn restore(
        mut local: ResourceCollection,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
        node: DataPathNode,
        _plugged: &ModuleCollection,
        _prev_version: Version,
    ) -> Result<Self> {
        tracing::trace!("restoring RpcAcceptorEngine states...");
        let state = *local
            .remove("state")
            .unwrap()
            .downcast::<State>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;
        let ops = *local
            .remove("ops")
            .unwrap()
            .downcast::<Box<Ops>>()
            .map_err(|x| anyhow!("fail to downcast, type_name={:?}", x.type_name()))?;

        let engine = AcceptorEngine {
            indicator: None,
            node,
            state,
            ops,
        };
        Ok(engine)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
}

use Status::Progress;

impl Engine for AcceptorEngine {
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        unsafe { Box::pin(self.get_unchecked_mut().mainloop()) }
    }

    fn description(&self) -> String {
        format!(
            "rpc_adapter::acceptor::AcceptorEngine, user: {}",
            self.state.shared.pid
        )
    }

    fn set_tracker(&mut self, indicator: Indicator) {
        self.indicator = Some(indicator);
    }

    fn set_els(&self) {
        let ops_ptr = self.ops.as_ref() as *const _;
        ulib::OPS.with(|ops| *ops.borrow_mut() = unsafe { Some(&*ops_ptr) })
    }
}

impl AcceptorEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut nwork = 0;
            let Progress(n) = self.check_new_incoming_connection().await?;
            nwork += n;
            // let Progress(n) = self.check_disconnected_events().await?;
            // nwork += n;
            if self.state.acceptor_should_stop() {
                return Ok(());
            }
            // if self.state.alive_engines() == 1 && self.num_alive_connections() == 0 {
            //     return Ok(());
            // }
            self.indicator.as_ref().unwrap().set_nwork(nwork);
            future::yield_now().await;
        }
    }
}

impl AcceptorEngine {
    async fn check_new_incoming_connection(&mut self) -> Result<Status, ControlPathError> {
        let table = self.state.resource().listener_table.inner().lock();
        if table.is_empty() {
            return Ok(Status::Progress(0));
        }
        for entry in table.values() {
            let listener = entry.data();
            if let Some(mut builder) = listener.1.try_get_request()? {
                // choose an rpc_adapter evenly
                let rpc_adapter_id = listener.0;
                let cq = self.state.resource().cq_ref_table.get(&rpc_adapter_id)?;
                // setup connection parameters
                let pre_id = builder
                    .set_send_cq(&cq)
                    .set_recv_cq(&cq)
                    .set_max_send_wr(128)
                    .set_max_recv_wr(128)
                    .build()?;
                // RpcAdapter please check for new pre_cmid
                self.state
                    .resource()
                    .pre_cmid_table
                    .entry(rpc_adapter_id)
                    .or_insert_with(VecDeque::new)
                    .push_back(pre_id);
            }
        }
        Ok(Status::Progress(1))
    }
}
