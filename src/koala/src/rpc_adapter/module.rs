use std::collections::VecDeque;
use std::os::unix::net::{SocketAddr, UCred};
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use lazy_static::lazy_static;
use nix::unistd::Pid;

use interface::engine::{EngineType, SchedulingMode};
use ipc;
use ipc::transport::rdma::control_plane;
use ipc::unix::DomainSocket;

use super::engine::{RpcAdapterEngine, TlStorage};
use super::state::State;
use crate::engine::manager::RuntimeManager;
use crate::node::Node;
use crate::state_mgr::StateManager;
use crate::transport::rdma::ops::Ops;

lazy_static! {
    pub(crate) static ref STATE_MGR: Arc<StateManager<State>> = Arc::new(StateManager::new());
}

pub(crate) struct RpcAdapterEngineBuilder {
    node: Node,
    client_pid: Pid,
    mode: SchedulingMode,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<ipc::mrpc::cmd::Command>,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<ipc::mrpc::cmd::Completion>,
    ops: Ops,
}

impl RpcAdapterEngineBuilder {
    fn new(
        node: Node,
        client_pid: Pid,
        mode: SchedulingMode,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<ipc::mrpc::cmd::Command>,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<ipc::mrpc::cmd::Completion>,
        ops: Ops,
    ) -> Self {
        RpcAdapterEngineBuilder {
            node,
            client_pid,
            mode,
            cmd_rx,
            cmd_tx,
            ops,
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
                ops: self.ops,
                state,
            }),
            flag: false,
            salloc: salloc_state,
            odp_mr: None,
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
        // NOTE(cjr): Why I am using rdma's control_plane request
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
        runtime_manager: &RuntimeManager,
        n: Node,
        mode: SchedulingMode,
        client_pid: Pid,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<ipc::mrpc::cmd::Command>,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<ipc::mrpc::cmd::Completion>,
    ) -> Result<RpcAdapterEngine> {
        let ops = crate::transport::rdma::module::create_ops(runtime_manager, client_pid)?;

        let builder = RpcAdapterEngineBuilder::new(n, client_pid, mode, cmd_rx, cmd_tx, ops);
        let engine = builder.build()?;

        Ok(engine)
    }
}
