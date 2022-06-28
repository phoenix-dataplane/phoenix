use std::os::unix::net::{SocketAddr, UCred};
use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use lazy_static::lazy_static;
use nix::unistd::Pid;
use uuid::Uuid;

use interface::engine::{EngineType, SchedulingMode};
use ipc::customer::{Customer, ShmCustomer};
use ipc::mrpc::{cmd, control_plane, dp};
use ipc::unix::DomainSocket;

use super::engine::MrpcEngine;
use super::state::State;
use crate::config::MrpcConfig;
use crate::engine::container::EngineContainer;
use crate::engine::manager::RuntimeManager;
use crate::mrpc::meta_pool::MessageMetaPool;
use crate::node::Node;
use crate::state_mgr::StateManager;

lazy_static! {
    static ref STATE_MGR: Arc<StateManager<State>> = Arc::new(StateManager::new());
}

pub type CustomerType =
    Customer<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>;

pub(crate) struct MrpcEngineBuilder {
    customer: CustomerType,
    node: Node,
    client_pid: Pid,
    mode: SchedulingMode,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<cmd::Command>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Completion>,
}

impl MrpcEngineBuilder {
    fn new(
        customer: CustomerType,
        node: Node,
        client_pid: Pid,
        mode: SchedulingMode,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<cmd::Command>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Completion>,
    ) -> Self {
        MrpcEngineBuilder {
            customer,
            cmd_tx,
            cmd_rx,
            node,
            client_pid,
            mode,
        }
    }

    fn build(self) -> Result<MrpcEngine> {
        const BUF_LEN: usize = 32;
        let state = STATE_MGR.get_or_create_state(self.client_pid)?;
        assert_eq!(self.node.engine_type, EngineType::Mrpc);

        Ok(MrpcEngine {
            _state: state,
            customer: self.customer,
            node: self.node,
            cmd_tx: self.cmd_tx,
            cmd_rx: self.cmd_rx,
            meta_pool: MessageMetaPool::new(),
            _mode: self.mode,
            transport_type: None,
            indicator: None,
            wr_read_buffer: Vec::with_capacity(BUF_LEN),
        })
    }
}

pub struct MrpcModule {
    config: MrpcConfig,
    runtime_manager: Arc<RuntimeManager>,
}

impl MrpcModule {
    pub fn new(config: MrpcConfig, runtime_manager: Arc<RuntimeManager>) -> Self {
        MrpcModule {
            config,
            runtime_manager,
        }
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
        node: Node,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<cmd::Command>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Completion>,
    ) -> Result<()> {
        // 1. generate a path and bind a unix domain socket to it
        let uuid = Uuid::new_v4();
        let instance_name = format!("{}-{}.sock", self.config.engine_basename, uuid);
        let engine_path = self.config.prefix.join(instance_name);

        // 2. create customer stub
        let customer =
            Customer::from_shm(ShmCustomer::accept(sock, client_path, mode, engine_path)?);

        // 3. the following part are expected to be done in the Engine's constructor.
        // the mrpc module is responsible for initializing and starting the mrpc engines
        let client_pid = Pid::from_raw(cred.pid.unwrap());

        // 4. create the engine
        let builder = MrpcEngineBuilder::new(customer, node, client_pid, mode, cmd_tx, cmd_rx);
        let engine = builder.build()?;

        // 5. submit the engine to a runtime
        self.runtime_manager
            .submit(EngineContainer::new(engine), mode);

        Ok(())
    }
}
