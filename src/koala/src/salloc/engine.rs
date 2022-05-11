use std::future::Future;
use std::io;
use std::os::unix::prelude::AsRawFd;

use ipc::customer::Customer;
use ipc::salloc::cmd;

use super::module::CustomerType;
use super::ControlPathError;
use crate::engine::{future, Engine, EngineResult, Indicator};
use crate::node::Node;

use crate::rpc_adapter;
use crate::rpc_adapter::state::State as RpcAdapterState;


pub struct SallocEngine {
    pub(crate) customer: CustomerType,
    pub(crate) node: Node,
    pub(crate) indicator: Option<Indicator>,
    pub(crate) adapter_state: RpcAdapterState,
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
        let api_guard = self.state.shared.api_engine.lock();
        match req {
            Command::AllocShm(size, _align) => {
                // TODO(wyj): implement backend heap allocator to properly handle align 
                let nbytes = size;
                log::trace!("AllocShm, nbytes: {}", nbytes);
                let pd = self.adapter_state.shared.resource.default_pds()[0];
                let access = rpc_adapter::ulib::uverbs::AccessFlags::REMOTE_READ
                    | rpc_adapter::ulib::uverbs::AccessFlags::REMOTE_WRITE
                    | rpc_adapter::ulib::uverbs::AccessFlags::LOCAL_WRITE;
                let mr: rpc_adapter::ulib::uverbs::MemoryRegion<u8> = pd.allocate(nbytes, access)?;
                let returned_mr = interface::returned::MemoryRegion {
                    handle: mr.inner,
                    rkey: mr.rkey(),
                    vaddr: mr.as_ptr() as u64,
                    map_len: mr.len() as u64,
                    file_off: mr.file_off,
                    pd: mr.pd().inner,
                };
                // mr's addr on backend side
                let remote_addr = mr.as_ptr() as u64;
                let file_off = mr.file_off as u64;
                self.adapter_state.shared.resource.insert_mr(mr)?;
                Ok(cmd::CompletionKind::AllocShm(
                    returned_mr,
                    memfd,
                ))
            }
            Command::DeallocShm(addr) => {
                // TODO(wyj): drop/remove the memory region from RpcAdapter's mr_table. 
                // DeregMr will be performed during drop.
                unimplemented!()
            }
        }
    }
}