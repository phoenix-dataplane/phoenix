use ipc::mrpc;

use interface::engine::SchedulingMode;

use super::state::State;
use super::ulib;
use super::module::ServiceType;
use super::{ControlPathError, DatapathError};
use crate::engine::{Engine, EngineStatus, Upgradable, Version, Vertex};
use crate::node::Node;

pub struct TlStorage {
    pub(crate) service: ServiceType,
    pub(crate) state: State,
}

pub struct RpcAdapterEngine {
    pub(crate) tls: Box<TlStorage>,

    pub(crate) node: Node,
    pub(crate) cmd_rx: std::sync::mpsc::Receiver<mrpc::cmd::Command>,
    pub(crate) cmd_tx: std::sync::mpsc::Sender<mrpc::cmd::Completion>,

    pub(crate) dp_spin_cnt: usize,
    pub(crate) backoff: usize,
    pub(crate) _mode: SchedulingMode,
}

impl Upgradable for RpcAdapterEngine {
    fn version(&self) -> Version {
        unimplemented!();
    }

    fn check_compatible(&self, _v2: Version) -> bool {
        unimplemented!();
    }

    fn suspend(&mut self) {
        unimplemented!();
    }

    fn dump(&self) {
        unimplemented!();
    }

    fn restore(&mut self) {
        unimplemented!();
    }
}

impl Vertex for RpcAdapterEngine {
    crate::impl_vertex_for_engine!(node);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for RpcAdapterEngine {
    fn resume(&mut self) -> Result<EngineStatus, Box<dyn std::error::Error>> {
        // check input queue
        self.check_input_queue()?;

        // check service
        self.check_transport_service()?;

        // check input command queue
        self.check_input_cmd_queue()?;
        Ok(EngineStatus::Continue)
    }

    #[inline]
    unsafe fn tls(&self) -> Option<&'static dyn std::any::Any> {
        // let (addr, meta) = (self.tls.as_ref() as *const TlStorage).to_raw_parts();
        // let tls: *const TlStorage = std::ptr::from_raw_parts(addr, meta);
        let tls = self.tls.as_ref() as *const TlStorage;
        Some(&*tls)
    }
}

impl RpcAdapterEngine {
    fn check_input_queue(&mut self) -> Result<Status, DatapathError> {
        Ok(Status::Progress(0))
    }

    fn check_transport_service(&mut self) -> Result<Status, DatapathError> {
        Ok(Status::Progress(0))
    }

    fn check_input_cmd_queue(&mut self) -> Result<Status, ControlPathError> {
        use std::sync::mpsc::TryRecvError;
        match self.cmd_rx.try_recv() {
            Ok(req) => {
                let result = self.process_cmd(&req);
                match result {
                    Ok(res) => self.cmd_tx.send(mrpc::cmd::Completion(Ok(res)))?,
                    Err(ControlPathError::InProgress) => return Ok(Progress(0)),
                    Err(e) => self.cmd_tx.send(mrpc::cmd::Completion(Err(e.into())))?,
                }
                Ok(Progress(1))
            }
            Err(TryRecvError::Empty) => Ok(Progress(0)),
            Err(TryRecvError::Disconnected) => Ok(Status::Disconnected),
        }
    }

    fn process_cmd(
        &self,
        req: &mrpc::cmd::Command,
    ) -> Result<mrpc::cmd::CompletionKind, ControlPathError> {
        match req {
            mrpc::cmd::Command::SetTransport(_) => {
                unimplemented!()
            }
            mrpc::cmd::Command::Connect(addr) => {
                log::trace!("Connect, addr: {:?}", addr);
                // create CmIdBuilder
                let builder = ulib::ucm::CmIdBuilder::new()
                    .set_max_send_wr(128)
                    .resolve_route(addr)?;
                let pre_id = builder.build()?;
                // connect
                let id = pre_id.connect(None)?;
                let handle = id.inner.handle.0;
                Ok(mrpc::cmd::CompletionKind::Connect(handle))
            }
        }
    }
}
