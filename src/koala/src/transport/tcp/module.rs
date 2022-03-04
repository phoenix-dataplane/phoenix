use std::collections::VecDeque;
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
use interface::engine::{SchedulingMode, EngineType};
use ipc;
use ipc::customer::{Customer, ShmCustomer};
use ipc::transport::tcp::{cmd, control_plane, dp};
use ipc::unix::DomainSocket;

use super::engine::TransportEngine;
use crate::node::Node;

pub type CustomerType =
    Customer<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>;

pub(crate) struct TransportEngineBuilder {
    customer: CustomerType,
    _client_pid: Pid,
    mode: SchedulingMode,
}

impl TransportEngineBuilder {
    fn new(
        customer: CustomerType,
        _client_pid: Pid,
        mode: SchedulingMode,
    ) -> Self {
        TransportEngineBuilder {
            customer,
            _client_pid,
            mode,
        }
    }

    fn build(self) -> Result<TransportEngine> {
        // create or get the state of the process
        // let state = STATE_MGR.get_or_create_state(self.client_pid)?;
        let node = Node::new(EngineType::TcpTransport);

        Ok(TransportEngine {
            customer: self.customer,
            node,
            cq_err_buffer: VecDeque::new(),
            dp_spin_cnt: 0,
            backoff: 1,
            _mode: self.mode,
            state: super::engine::State::new(),
            cmd_buffer: None,
            last_cmd_ts: Instant::now(),
        })
    }
}

pub struct TransportModule {
    runtime_manager: Arc<RuntimeManager>,
}

impl TransportModule {
    pub fn new(runtime_manager: Arc<RuntimeManager>) -> Self {
        TransportModule { runtime_manager }
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
        let engine_path = PathBuf::from(format!("/tmp/koala/koala-transport-engine-{}.sock", uuid));

        // 2. create customer stub
        let customer = Customer::from_shm(ShmCustomer::accept(sock, client_path, mode, engine_path)?);

        // 3. the following part are expected to be done in the Engine's constructor.
        // the transport module is responsible for initializing and starting the transport engines
        let client_pid = Pid::from_raw(cred.pid.unwrap());

        let builder = TransportEngineBuilder::new(customer, client_pid, mode);
        let engine = builder.build()?;

        // 5. submit the engine to a runtime
        self.runtime_manager.submit(Box::new(engine), mode);

        Ok(())
    }
}
