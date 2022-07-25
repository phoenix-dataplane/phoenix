use std::collections::VecDeque;
use std::os::unix::net::{SocketAddr, UCred};
use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use nix::unistd::Pid;
use uuid::Uuid;

use interface::engine::{EngineType, SchedulingMode};
use ipc;
use ipc::customer::{Customer, ShmCustomer};
use ipc::transport::rdma::{cmd, control_plane, dp};
use ipc::unix::DomainSocket;

use super::cm::engine::CmEngine;
use super::engine::TransportEngine;
use super::ops::Ops;
use super::state::{Shared, State};
use crate::config::RdmaTransportConfig;
use crate::engine::container::EngineContainer;
use crate::engine::manager::RuntimeManager;
use crate::module::Service;
use crate::node::Node;
use crate::state_mgr::SharedStateManager;

pub type CustomerType =
    Customer<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>;

/// Create API Operations.
pub(crate) fn create_ops(
    runtime_manager: &Arc<RuntimeManager>,
    state_mgr: &mut SharedStateManager<Shared>,
    client_pid: Pid,
) -> Result<Ops> {
    // first create cm engine
    create_cm_engine(runtime_manager, state_mgr, client_pid)?;

    // create or get the state of the process
    let shared = state_mgr.get_or_create(client_pid)?;
    let state = State::new(shared);

    Ok(Ops::new(state))
}

fn create_cm_engine(
    runtime_manager: &Arc<RuntimeManager>,
    state_mgr: &mut SharedStateManager<Shared>,
    client_pid: Pid,
) -> Result<()> {
    let shared = state_mgr.get_or_create(client_pid)?;

    // only create one cm_engine for a client process
    // if refcnt > 1, then there is already a CmEngine running
    if Arc::strong_count(&shared) > 1 {
        return Ok(());
    }

    let state = State::new(shared);
    let node = Node::new(EngineType::RdmaConnMgmt);
    let cm_engine = CmEngine::new(node, state);

    // always submit the engine to a dedicate runtime
    let gid = runtime_manager.get_new_group_id(client_pid, Service(String::from("DEFAULT")));
    runtime_manager.submit(
        client_pid,
        gid,
        EngineContainer::new(cm_engine),
        SchedulingMode::Dedicate,
    );
    Ok(())
}

pub(crate) struct TransportEngineBuilder {
    customer: CustomerType,
    mode: SchedulingMode,
    ops: Ops,
}

impl TransportEngineBuilder {
    fn new(customer: CustomerType, mode: SchedulingMode, ops: Ops) -> Self {
        TransportEngineBuilder {
            customer,
            mode,
            ops,
        }
    }

    fn build(self) -> Result<TransportEngine> {
        const BUF_LEN: usize = 32;
        // create or get the state of the process
        let node = Node::new(EngineType::RdmaTransport);

        Ok(TransportEngine {
            customer: self.customer,
            node,
            indicator: None,
            _mode: self.mode,
            ops: self.ops,
            cq_err_buffer: VecDeque::new(),
            wr_read_buffer: Vec::with_capacity(BUF_LEN),
        })
    }
}

pub struct TransportModule {
    pub(crate) state_mgr: SharedStateManager<Shared>,
    config: RdmaTransportConfig,
    runtime_manager: Arc<RuntimeManager>,
}

impl TransportModule {
    pub fn new(config: RdmaTransportConfig, runtime_manager: Arc<RuntimeManager>) -> Self {
        TransportModule {
            state_mgr: SharedStateManager::new(),
            config,
            runtime_manager,
        }
    }

    pub fn handle_request(
        &self,
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

        // 3.1. create the ops and cm engine
        let ops = create_ops(&self.runtime_manager, &mut self.state_mgr, client_pid)?;

        // 4. create the engine
        let builder = TransportEngineBuilder::new(customer, mode, ops);
        let engine = builder.build()?;

        // submit the engine to a runtime
        let gid = self
            .runtime_manager
            .get_new_group_id(client_pid, Service(String::from("DEFAULT")));
        self.runtime_manager
            .submit(client_pid, gid, EngineContainer::new(engine), mode);

        Ok(())
    }
}
