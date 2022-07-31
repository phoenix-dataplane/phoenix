use std::collections::VecDeque;
use std::pin::Pin;

use futures::future::BoxFuture;

use super::super::engine::{TlStorage, ELS};
use super::super::state::State;
use super::super::ControlPathError;
use crate::engine::{future, Engine, EngineLocalStorage, EngineResult, Indicator};
use crate::node::Node;

pub struct AcceptorEngine {
    pub(crate) node: Node,
    pub(crate) indicator: Indicator,
    pub(crate) state: State,
    pub(crate) tls: Box<TlStorage>,
}

impl AcceptorEngine {
    pub(crate) fn new(node: Node, state: State, tls: Box<TlStorage>) -> Self {
        Self {
            node,
            indicator: Default::default(),
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
        // TODO(cjr): add doc
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
            }
        }
        Ok(Status::Progress(1))
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
