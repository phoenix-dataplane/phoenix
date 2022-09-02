use std::collections::VecDeque;
use std::future::Future;
use std::os::unix::io::AsRawFd;

use super::super::engine::TlStorage;
use super::super::state::State;
use super::super::ControlPathError;
use interface::AsHandle;

use crate::engine::{future, Engine, EngineLocalStorage, EngineResult, Indicator};
use crate::node::Node;
use crate::tcp_rpc_adapter::get_ops;
use crate::transport::tcp::ops::CompletionQueue;

pub struct AcceptorEngine {
    pub(crate) node: Node,
    pub(crate) indicator: Option<Indicator>,
    pub(crate) state: State,
    pub(crate) tls: Box<TlStorage>,
    pub(crate) rpc_adapter_id: usize,
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
        format!(
            "tcp_rpc_adapter::acceptor::AcceptorEngine, user: {}",
            self.state.shared.pid
        )
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
    fn check_new_incoming_connection(&mut self) -> Result<Status, ControlPathError> {
        for listener in get_ops().state.listener_table.values() {
            match listener.accept() {
                Ok(tuple) => {
                    let (sock, addr) = tuple;
                    sock.set_nonblocking(true);
                    let handle = sock.as_raw_fd().as_handle();
                    get_ops().state.conn_table.insert(handle, sock);
                    get_ops()
                        .state
                        .cq_table
                        .insert(handle, CompletionQueue::new());
                }
                Err(_e) => continue,
            }
        }
        Ok(Status::Progress(1))
    }
}
