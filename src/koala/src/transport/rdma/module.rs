use std::collections::VecDeque;
use std::os::unix::net::{SocketAddr, UCred};
use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use lazy_static::lazy_static;
use nix::unistd::Pid;
use uuid::Uuid;

use interface::engine::{EngineType, SchedulingMode};
use ipc;
use ipc::customer::{Customer, ShmCustomer};
use ipc::transport::rdma::{cmd, control_plane, dp};
use ipc::unix::DomainSocket;

use super::engine::TransportEngine;
use super::state::State;
use crate::engine::manager::RuntimeManager;
use crate::engine::container::EngineContainer;
use crate::node::Node;
use crate::state_mgr::StateManager;
use crate::config::RdmaTransportConfig;

lazy_static! {
    static ref STATE_MGR: Arc<StateManager<State>> = Arc::new(StateManager::new());
}

pub type CustomerType =
    Customer<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>;

pub(crate) struct TransportEngineBuilder {
    customer: CustomerType,
    client_pid: Pid,
    mode: SchedulingMode,
}

impl TransportEngineBuilder {
    fn new(customer: CustomerType, client_pid: Pid, mode: SchedulingMode) -> Self {
        TransportEngineBuilder {
            customer,
            client_pid,
            mode,
        }
    }

    fn build(self) -> Result<TransportEngine> {
        // create or get the state of the process
        let state = STATE_MGR.get_or_create_state(self.client_pid)?;
        let node = Node::new(EngineType::RdmaTransport);

        Ok(TransportEngine {
            customer: self.customer,
            node,
            cq_err_buffer: VecDeque::new(),
            _mode: self.mode,
            state,
            cmd_buffer: None,
            indicator: None,
        })
    }
}

pub struct TransportModule {
    config: RdmaTransportConfig,
    runtime_manager: Arc<RuntimeManager>,
}

impl TransportModule {
    pub fn new(config: RdmaTransportConfig, runtime_manager: Arc<RuntimeManager>) -> Self {
        TransportModule { config, runtime_manager }
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

    pub(crate) fn create_engine(
        &mut self,
        customer: CustomerType,
        mode: SchedulingMode,
        client_pid: Pid,
    ) -> Result<TransportEngine> {
        let builder = TransportEngineBuilder::new(customer, client_pid, mode);
        let engine = builder.build()?;

        Ok(engine)
    }

    pub fn handle_new_client<P: AsRef<Path>>(
        &mut self,
        sock: &DomainSocket,
        client_path: P,
        mode: SchedulingMode,
        cred: &UCred,
    ) -> Result<()> {
        // create a new transport engine
        // 1. generate a path and bind a unix domain socket to it
        let uuid = Uuid::new_v4();
        let instance_name = format!("{}-{}.sock", self.config.engine_basename, uuid);
        let engine_path = self.config.prefix.join(instance_name);

        // 2. create customer stub
        let customer =
            Customer::from_shm(ShmCustomer::accept(sock, client_path, mode, engine_path)?);

        // 3. the following part are expected to be done in the Engine's constructor.
        // the transport module is responsible for initializing and starting the transport engines
        let client_pid = Pid::from_raw(cred.pid.unwrap());

        // 4. create the engine
        let builder = TransportEngineBuilder::new(customer, client_pid, mode);
        let engine = builder.build()?;

        // submit the engine to a runtime
        self.runtime_manager.submit(EngineContainer::new(engine), mode);

        Ok(())
    }
}
