use std::os::unix::net::{SocketAddr, UCred};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::Result;
use nix::unistd::Pid;
use uuid::Uuid;

use engine::manager::RuntimeManager;
use interface::engine::{EngineType, SchedulingMode};
use ipc;
use ipc::service::{SerivceFlavor, Service};
use ipc::transport::rdma::{cmd, control_plane, dp};
use ipc::unix::DomainSocket;

use super::engine::RpcAdapterEngine;
use crate::node::Node;

pub(crate) struct RpcAdapterEngineBuilder {
    service: Service<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
    client_pid: Pid,
    mode: SchedulingMode,
}

impl RpcAdapterEngineBuilder {
    fn new(
        service: Service<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
        client_pid: Pid,
        mode: SchedulingMode,
    ) -> Self {
        RpcAdapterEngineBuilder {
            service,
            client_pid,
            mode,
        }
    }

    fn build(self) -> Result<RpcAdapterEngine> {
        // create or get the state of the process
        // let state = STATE_MGR.get_or_create_state(self.client_pid)?;
        let node = Node::new(EngineType::RpcAdapter);

        Ok(RpcAdapterEngine {
            service: self.service,
            node,
            dp_spin_cnt: 0,
            backoff: 1,
            _mode: self.mode,
        })
    }
}

pub struct RpcAdapterModule {
    runtime_manager: Arc<RuntimeManager>,
}

impl RpcAdapterModule {
    pub fn handle_request(
        &mut self,
        req: &control_plane::Request,
        _sock: &DomainSocket,
        sender: &SocketAddr,
        _cred: &UCred,
    ) -> Result<()> {
        let _client_path = sender
            .as_pathname()
            .ok_or_else(|| anyhow!("peer is unnamed, something is wrong"))?;
        match req {
            _ => unreachable!("unknown req: {:?}", req),
        }
    }

    pub(crate) fn create_engine<P: AsRef<Path>>(
        &mut self,
        sock: &DomainSocket,
        client_path: P,
        mode: SchedulingMode,
        client_pid: Pid,
    ) -> Result<RpcAdapterEngine> {
        // 1. generate a path and bind a unix domain socket to it
        let uuid = Uuid::new_v4();
        // TODO(cjr): make this configurable
        let engine_path =
            PathBuf::from(format!("/tmp/koala/koala-rpc_adapter-engine-{}.sock", uuid));

        // 2. create customer stub
        // let customer = Customer::accept(sock, client_path, mode, engine_path)?;
        let service = Service {
            flavor: SerivceFlavor::Sequential::new(),
        };

        // 3. create the engine
        let builder = RpcAdapterEngineBuilder::new(service, client_pid, mode);
        let engine = builder.build()?;

        Ok(engine)
    }
}
