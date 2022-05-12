use std::collections::VecDeque;
use std::sync::Arc;
use std::os::unix::net::{SocketAddr, UCred};

use anyhow::anyhow;
use anyhow::Result;
use lazy_static::lazy_static;
use nix::unistd::Pid;

use interface::engine::{EngineType, SchedulingMode};
use ipc;
use ipc::service::Service;
use ipc::transport::rdma::{cmd, control_plane, dp};
use ipc::unix::DomainSocket;

use super::engine::{RpcAdapterEngine, TlStorage};
use super::state::State;
use crate::node::Node;
use crate::state_mgr::StateManager;
use crate::transport::rdma::engine::TransportEngine;

lazy_static! {
    pub(crate) static ref STATE_MGR: Arc<StateManager<State>> = Arc::new(StateManager::new());
}

pub(crate) type ServiceType =
    Service<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>;

pub(crate) struct RpcAdapterEngineBuilder {
    node: Node,
    service: ServiceType,
    client_pid: Pid,
    mode: SchedulingMode,
    cmd_rx: std::sync::mpsc::Receiver<ipc::mrpc::cmd::Command>,
    cmd_tx: std::sync::mpsc::Sender<ipc::mrpc::cmd::Completion>,
    api_engine: TransportEngine,
}

impl RpcAdapterEngineBuilder {
    fn new(
        node: Node,
        service: ServiceType,
        client_pid: Pid,
        mode: SchedulingMode,
        cmd_rx: std::sync::mpsc::Receiver<ipc::mrpc::cmd::Command>,
        cmd_tx: std::sync::mpsc::Sender<ipc::mrpc::cmd::Completion>,
        api_engine: TransportEngine,
    ) -> Self {
        RpcAdapterEngineBuilder {
            node,
            service,
            client_pid,
            mode,
            cmd_rx,
            cmd_tx,
            api_engine,
        }
    }

    fn build(self) -> Result<RpcAdapterEngine> {
        // create or get the state of the process
        let state = STATE_MGR.get_or_create_state(self.client_pid)?;
        let salloc_state = crate::salloc::module::STATE_MGR.get_or_create_state(self.client_pid)?;
        assert_eq!(self.node.engine_type, EngineType::RpcAdapter);

        // The cq can only be created lazily. Because the engine on other side is not run at this
        // time.
        //// let pd = state.resource().default_pds()[0];
        // let ctx_list = ulib::uverbs::get_default_verbs_contexts(&self.service)?;
        // let ctx = &ctx_list[0];
        // let cq = ctx.create_cq(1024, 0)?;

        Ok(RpcAdapterEngine {
            tls: Box::new(TlStorage {
                service: self.service,
                api_engine: self.api_engine,
                state,
            }),
            salloc: salloc_state,
            cq: None,
            recent_listener_handle: None,
            local_buffer: VecDeque::new(),
            node: self.node,
            cmd_rx: self.cmd_rx,
            cmd_tx: self.cmd_tx,
            _mode: self.mode,
            indicator: None,
        })
    }
}

pub struct RpcAdapterModule;

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

    pub(crate) fn create_engine(
        n: Node,
        service: ServiceType,
        mode: SchedulingMode,
        client_pid: Pid,
        cmd_rx: std::sync::mpsc::Receiver<ipc::mrpc::cmd::Command>,
        cmd_tx: std::sync::mpsc::Sender<ipc::mrpc::cmd::Completion>,
        api_engine: TransportEngine,
    ) -> Result<RpcAdapterEngine> {
        let builder = RpcAdapterEngineBuilder::new(n, service, client_pid, mode, cmd_rx, cmd_tx, api_engine);
        let engine = builder.build()?;

        Ok(engine)
    }
}
