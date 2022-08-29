use std::collections::VecDeque;
use std::pin::Pin;

use anyhow::Result;
use futures::future::BoxFuture;

use super::super::engine::{TlStorage, ELS};
use super::super::state::State;
use super::super::ControlPathError;

use koala::engine::datapath::DataPathNode;
use koala::engine::{future, Decompose, Engine, EngineResult, Indicator};
use koala::envelop::ResourceDowncast;
use koala::impl_vertex_for_engine;
use koala::log;
use koala::module::{ModuleCollection, Version};
use koala::storage::{ResourceCollection, SharedStorage};

pub struct AcceptorEngine {
    pub(crate) indicator: Indicator,
    pub(crate) node: DataPathNode,
    pub(crate) state: State,
    pub(crate) tls: Box<TlStorage>,
}

impl_vertex_for_engine!(AcceptorEngine, node);

impl AcceptorEngine {
    pub(crate) fn new(node: DataPathNode, state: State, tls: Box<TlStorage>) -> Self {
        Self {
            indicator: Default::default(),
            node,
            state,
            tls,
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

        log::debug!("dumping RpcAdapter-AcceptorEngine states...");
        collections.insert("state".to_string(), Box::new(engine.state));
        collections.insert("tls".to_string(), Box::new(engine.tls));
        (collections, engine.node)
    }
}

impl AcceptorEngine {
    pub(crate) unsafe fn restore(
        mut local: ResourceCollection,
        _shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
        node: DataPathNode,
        _plugged: &ModuleCollection,
        _prev_version: Version,
    ) -> Result<Self> {
        log::debug!("restoring RpcAcceptorEngine states...");
        let state = *local
            .remove("state")
            .unwrap()
            .downcast_unchecked::<State>();
        let tls = *local
            .remove("tls")
            .unwrap()
            .downcast_unchecked::<Box<TlStorage>>();

        let engine = AcceptorEngine {
            indicator: Default::default(),
            node,
            state,
            tls,
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
    fn description(self: Pin<&Self>) -> String {
        format!(
            "rpc_adapter::acceptor::AcceptorEngine, user: {}",
            self.get_ref().state.shared.pid
        )
    }

    #[inline]
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator {
        &mut self.get_mut().indicator
    }

    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult> {
        Box::pin(async move { self.get_mut().mainloop().await })
    }

    #[inline]
    fn set_els(self: Pin<&mut Self>) {
        let tls = self.get_mut().tls.as_ref() as *const TlStorage;
        // SAFETY: This is fine here because ELS is only used while the engine is running.
        // As long as we do not move out or drop self.tls, we are good.
        ELS.with_borrow_mut(|els| *els = unsafe { Some(&*tls) });
    }
}

impl AcceptorEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut nwork = 0;
            let Progress(n) = self.check_new_incoming_connection().await?;
            nwork += n;
            if self.state.acceptor_should_stop() {
                log::debug!("{} is stopping", Pin::new(self).as_ref().description());
                return Ok(());
            }
            self.indicator.set_nwork(nwork);
            future::yield_now().await;
        }
    }
}

impl AcceptorEngine {
    async fn check_new_incoming_connection(&mut self) -> Result<Status, ControlPathError> {
        let table = &self.state.resource().listener_table.inner();
        // Fix a weird bug related to live upgrade
        if table.is_empty() {
            return Ok(Status::Progress(0));
        }
        let mut nwork = 0;
        for entry in table.iter() {
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
                    .set_max_inline_data(super::super::engine::MAX_INLINE_DATA as _)
                    .build()?;
                // RpcAdapter please check for new pre_cmid
                self.state
                    .resource()
                    .pre_cmid_table
                    .entry(rpc_adapter_id)
                    .or_insert_with(VecDeque::new)
                    .push_back(pre_id);
                nwork += 1;
            }
        }
        Ok(Status::Progress(nwork))
    }

    // async fn check_new_incoming_connection(&mut self) -> Result<Status, ControlPathError> {
    //     let table = self.state.resource().listener_table.inner().lock();
    //     for entry in table.values() {
    //         let listener = entry.data();
    //         if let Some(mut builder) = listener.1.try_get_request()? {
    //             // choose an rpc_adapter evenly
    //             let rpc_adapter_id = listener.0;
    //             let cq = self.state.resource().cq_ref_table.get(&rpc_adapter_id)?;
    //             // setup connection parameters
    //             let pre_id = builder
    //                 .set_send_cq(&cq)
    //                 .set_recv_cq(&cq)
    //                 .set_max_send_wr(128)
    //                 .set_max_recv_wr(128)
    //                 .build()?;
    //             // RpcAdapter please check for new pre_cmid
    //             self.state
    //                 .resource()
    //                 .pre_cmid_table
    //                 .entry(rpc_adapter_id)
    //                 .or_insert_with(VecDeque::new)
    //                 .push_back(pre_id);
    //         }
    //     }
    //     Ok(Status::Progress(1))
    // }
}
