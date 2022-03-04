use std::os::unix::net::{SocketAddr, UCred};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::Result;
use nix::unistd::Pid;
use uuid::Uuid;

use interface::engine::{EngineType, SchedulingMode};
use engine::manager::RuntimeManager;
use ipc::mrpc::{cmd, control_plane, dp};
use ipc::unix::DomainSocket;

use super::engine::MrpcEngine;
use crate::node::Node;

pub(crate) struct MrpcEngineBuilder {
    customer: ShmCustomer<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
    node: Node,
    _client_pid: Pid,
    mode: SchedulingMode,
}

impl MrpcEngineBuilder {
    fn new(
        customer: ShmCustomer<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
        node: Node,
        _client_pid: Pid,
        mode: SchedulingMode,
    ) -> Self {
        MrpcEngineBuilder {
            customer,
            node,
            _client_pid,
            mode,
        }
    }

    fn build(self) -> Result<MrpcEngine> {
        // let state = STATE_MGR.get_or_create_state(self.client_pid)?;

        Ok(MrpcEngine {
            customer: self.customer,
            node: self.node,
            dp_spin_cnt: 0,
            backoff: 1,
            _mode: self.mode,
            transport_type: None,
            cmd_buffer: None,
            last_cmd_ts: Instant::now(),
        })
    }
}

pub struct MrpcModule {
    runtime_manager: Arc<RuntimeManager>,
}

impl MrpcModule {
    pub fn new(runtime_manager: Arc<RuntimeManager>) -> Self {
        MrpcModule { runtime_manager }
    }

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

    pub fn handle_new_client<P: AsRef<Path>>(
        &mut self,
        sock: &DomainSocket,
        client_path: P,
        mode: SchedulingMode,
        cred: &UCred,
    ) -> Result<()> {
        // 1. generate a path and bind a unix domain socket to it
        let uuid = Uuid::new_v4();
        // TODO(cjr): make this configurable
        let engine_path = PathBuf::from(format!("/tmp/koala/koala-mrpc-engine-{}.sock", uuid));

        // 2. create customer stub
        let customer = ShmCustomer::accept(sock, client_path, mode, engine_path)?;

        // 3. the following part are expected to be done in the Engine's constructor.
        // the mrpc module is responsible for initializing and starting the mrpc engines
        let client_pid = Pid::from_raw(cred.pid.unwrap());

        // 4. create the engine
        let node = Node::new(EngineType::Mrpc);
        let builder = MrpcEngineBuilder::new(customer, node, client_pid, mode);
        let engine = builder.build()?;

        // 5. submit the engine to a runtime
        self.runtime_manager.submit(Box::new(engine), mode);

        Ok(())
    }
}
