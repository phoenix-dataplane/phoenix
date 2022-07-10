use std::pin::Pin;
use std::collections::VecDeque;

use futures::future::BoxFuture;

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

    // fn num_alive_connections(&self) -> usize {
    //     self.state.resource().cmid_table.inner().lock().len()
    // }

    // should have a loop to get whatever event and process them
    // async fn check_disconnected_events(&mut self) -> Result<Status, ControlPathError> {
    //     use interface::AsHandle;

    //     log::warn!("check_disconnected_events: {}", self.num_alive_connections());
    //     let table = self.state.resource().cmid_table.inner().lock();
    //     let mut closing = Vec::new();
    //     for entry in table.values() {
    //         let conn_ctx = entry.data();
    //         let cmid = &conn_ctx.cmid;
    //         let ec = cmid.event_channel();
    //         let res =
    //             ec.try_get_cm_event(rdma::ffi::rdma_cm_event_type::RDMA_CM_EVENT_DISCONNECTED);
    //         if res.is_none() {
    //             continue;
    //         }
    //         log::warn!("check_disconnected_events: {:?}", cmid);
    //         if res.as_ref().unwrap().is_err() {
    //             log::warn!("try_get_cm_event: cmid: {:?}, ec: {:?}", cmid, ec);
    //         }
    //         let event = res.unwrap()?;
    //         drop(event);
    //         cmid.disconnect().await?;
    //         closing.push(cmid.as_handle());
    //     }
    //     for cmid_handle in closing {
    //         log::debug!("closing cmid_handle: {:?}", cmid_handle);
    //         self.state
    //             .resource()
    //             .cmid_table
    //             .close_resource(&cmid_handle)?;
    //     }
    //     Ok(Status::Progress(1))
    // }
}
