use std::collections::VecDeque;
use std::future::Future;

use super::super::engine::TlStorage;
use super::super::state::State;
use super::super::ControlPathError;
use crate::engine::{future, Engine, EngineLocalStorage, EngineResult, Indicator};
use crate::node::Node;

pub struct AcceptorEngine {
    pub(crate) node: Node,
    pub(crate) indicator: Option<Indicator>,
    pub(crate) state: State,
    pub(crate) tls: Box<TlStorage>,
}

impl AcceptorEngine {
    pub(crate) fn new(node: Node, state: State, tls: Box<TlStorage>) -> Self {
        Self {
            node,
            indicator: None,
            state,
            tls,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
}

use Status::Progress;

crate::unimplemented_ungradable!(AcceptorEngine);
crate::impl_vertex_for_engine!(AcceptorEngine, node);

impl Engine for AcceptorEngine {
    type Future = impl Future<Output = EngineResult>;

    fn description(&self) -> String {
        format!("rpc_adapter::acceptor::AcceptorEngine")
    }

    fn set_tracker(&mut self, indicator: Indicator) {
        self.indicator = Some(indicator);
    }

    fn entry(mut self) -> Self::Future {
        Box::pin(async move { self.mainloop().await })
    }

    #[inline]
    unsafe fn els(&self) -> Option<&'static dyn EngineLocalStorage> {
        let tls = self.tls.as_ref() as *const TlStorage;
        Some(&*tls)
    }
}

impl AcceptorEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut nwork = 0;
            let Progress(n) = self.check_new_incoming_connection().await?;
            nwork += n;
            if self.state.acceptor_should_stop() {
                return Ok(());
            }
            self.indicator.as_ref().unwrap().set_nwork(nwork);
            future::yield_now().await;
        }
    }
}

impl AcceptorEngine {
    async fn check_new_incoming_connection(&mut self) -> Result<Status, ControlPathError> {
        let table = self.state.resource().listener_table.inner().lock();
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
