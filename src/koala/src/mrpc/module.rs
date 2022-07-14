use std::os::unix::net::{SocketAddr, UCred};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use nix::unistd::Pid;
use uuid::Uuid;

use interface::engine::{EngineType, SchedulingMode};
use ipc::customer::{Customer, ShmCustomer};
use ipc::mrpc::{cmd, control_plane, dp};
use ipc::unix::DomainSocket;

use super::engine::MrpcEngine;
use super::state::{State, Shared};
use crate::config::MrpcConfig;
use crate::engine::container::EngineContainer;
use crate::engine::manager::RuntimeManager;
use crate::mrpc::meta_pool::MetaBufferPool;
use crate::node::Node;
use crate::state_mgr::SharedStateManager;


pub type CustomerType =
    Customer<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>;

pub(crate) struct MrpcEngineBuilder {
    customer: CustomerType,
    node: Node,
    client_pid: Pid,
    mode: SchedulingMode,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<cmd::Command>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Completion>,
    serializer_build_cache: PathBuf,
    shared: Arc<Shared>,
}

impl MrpcEngineBuilder {
    fn new(
        customer: CustomerType,
        node: Node,
        client_pid: Pid,
        mode: SchedulingMode,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<cmd::Command>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Completion>,
        serializer_build_cache: PathBuf,
        shared: Arc<Shared>,
    ) -> Self {
        MrpcEngineBuilder {
            customer,
            cmd_tx,
            cmd_rx,
            node,
            client_pid,
            mode,
            serializer_build_cache,
            shared
        }
    }

    fn build(self) -> Result<MrpcEngine> {
        const META_BUFFER_POOL_CAP: usize = 128;
        const BUF_LEN: usize = 32;

        let state = State::new(self.shared);

        assert_eq!(self.node.engine_type, EngineType::Mrpc);

        Ok(MrpcEngine {
            _state: state,
            customer: self.customer,
            node: self.node,
            cmd_tx: self.cmd_tx,
            cmd_rx: self.cmd_rx,
            meta_buf_pool: MetaBufferPool::new(META_BUFFER_POOL_CAP),
            _mode: self.mode,
            dispatch_build_cache: self.serializer_build_cache,
            transport_type: None,
            indicator: None,
            wr_read_buffer: Vec::with_capacity(BUF_LEN),
        })
    }
}

pub struct MrpcModule {
    stage_mgr: SharedStateManager<Shared>,
    config: MrpcConfig,
    runtime_manager: Arc<RuntimeManager>,
}

impl MrpcModule {
    pub fn new(config: MrpcConfig, runtime_manager: Arc<RuntimeManager>) -> Self {
        if !config.build_cache.is_dir() {
            std::fs::create_dir(&config.build_cache).unwrap();
        }
        MrpcModule {
            stage_mgr: SharedStateManager::new(),
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
        &self,
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

        let shared = self.stage_mgr.get_or_create(client_pid)?;

        // 4. create the engine
        let builder = MrpcEngineBuilder::new(
            customer,
            node,
            client_pid,
            mode,
            cmd_tx,
            cmd_rx,
            self.config.build_cache.clone(),
            shared,
        );

        let engine = builder.build()?;

        // 5. submit the engine to a runtime
        self.runtime_manager
            .submit(EngineContainer::new(engine), mode);

        Ok(())
    }
}
