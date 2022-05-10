use std::future::Future;
use std::io;

use ipc::customer::Customer;
use ipc::salloc::cmd;

use super::module::CustomerType;
use super::ControlPathError;
use crate::engine::{future, Engine, EngineResult, Indicator};
use crate::node::Node;
use crate::rpc_adapter::state::State;

pub struct SallocEngine {
    pub(crate) customer: CustomerType,
    pub(crate) node: Node,
    pub(crate) indicator: Option<Indicator>,
    pub(crate) state: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

crate::unimplemented_ungradable!(SallocEngine);
crate::impl_vertex_for_engine!(SallocEngine, node);

impl Engine for SallocEngine {
    type Future = impl Future<Output = EngineResult>;

    fn description(&self) -> String {
        format!("SallocEngine")
    }

    fn set_tracker(&mut self, indicator: Indicator) {
        self.indicator = Some(indicator);
    }

    fn entry(mut self) -> Self::Future {
        Box::pin(async move { self.mainloop().await })
    }
}

impl SallocEngine {
    async fn mainloop(&mut self) -> EngineResult {
        loop {
            let mut nwork = 0;
            if let Progress(n) = self.check_cmd()? {
                nwork += n;
            }
            self.indicator.as_ref().unwrap().set_nwork(nwork);
            future::yield_now().await;
        }
    }
}

impl SallocEngine {
    fn check_cmd(&mut self) -> Result<Status, ControlPathError> {
        match self.customer.try_recv_cmd() {
            Ok(req) => {
                let result = self.process_cmd(req);
                match result {
                    Ok(res) => self.customer.send_comp(cmd::Completion(Ok(res)))?,
                    Err(e) => self.customer.send_comp(cmd::Completion(Err(e.into())))?,
                }
                Ok(Progress(1))
            }
            Err(ipc::TryRecvError::Empty) => {
                // do nothing
                Ok(Progress(0))
            }
            Err(ipc::TryRecvError::Disconnected) => Ok(Status::Disconnected),
            Err(ipc::TryRecvError::Other(_e)) => Err(ControlPathError::IpcTryRecv),
        }
    }

    fn process_cmd(&mut self, req: cmd::Command) -> Result<cmd::CompletionKind, ControlPathError> {
        use cmd::{Command, CompletionKind};
        match req {
            Command::AllocShm(size, align) => {
            }
            Command::DeallocShm(addr) => {
            }
        }
    }
}
