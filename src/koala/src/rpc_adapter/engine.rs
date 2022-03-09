use std::mem;

use ipc::mrpc;
use ipc::transport::rdma as transport;

use interface::addrinfo::{AddrFamily, AddrInfo, AddrInfoFlags, AddrInfoHints, PortSpace};
use interface::engine::SchedulingMode;

use super::module::ServiceType;
use super::{rx_recv_impl, ControlPathError, DatapathError};
use crate::engine::{Engine, EngineStatus, Upgradable, Version, Vertex};
use crate::node::Node;

pub struct RpcAdapterEngine {
    pub(crate) service: ServiceType,

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
                // cread_id
                let req = transport::cmd::Command::CreateId(PortSpace::TCP);
                self.service.send_cmd(req)?;
                let cmid = rx_recv_impl!(
                    self.service,
                    transport::cmd::CompletionKind::CreateId,
                    cmid,
                    { Ok(cmid) }
                )?;
                let drop_cmid = DropCmId(&self.service, cmid.handle);
                assert!(cmid.qp.is_none());
                // resolve_addr
                let req = transport::cmd::Command::ResolveAddr(cmid.handle.0, *addr);
                self.service.send_cmd(req)?;
                rx_recv_impl!(self.service, transport::cmd::CompletionKind::ResolveAddr)?;
                // resolve_route
                let req = transport::cmd::Command::ResolveRoute(cmid.handle.0, 2000); 
                self.service.send_cmd(req)?;
                rx_recv_impl!(self.service, transport::cmd::CompletionKind::ResolveRoute)?;
                // connect
                let req = transport::cmd::Command::Connect(cmid.handle.0, None);
                self.service.send_cmd(req)?;
                rx_recv_impl!(self.service, transport::cmd::CompletionKind::Connect)?;
                // finally connected
                mem::forget(drop_cmid);
                let handle = cmid.handle.0;
                Ok(mrpc::cmd::CompletionKind::Connect(handle))
            }
        }
    }
}

struct DropCmId<'a>(&'a ServiceType, interface::CmId);

impl<'a> Drop for DropCmId<'a> {
    fn drop(&mut self) {
        (|| {
            let req = transport::cmd::Command::DestroyId(self.1);
            self.0.send_cmd(req)?;
            rx_recv_impl!(self.0, transport::cmd::CompletionKind::DestroyId)?;
            Ok(())
        })()
        .unwrap_or_else(|e: ControlPathError| eprintln!("Destroying CmId: {}", e));
    }
}
