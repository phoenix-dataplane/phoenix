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

use super::acceptor::engine::AcceptorEngine;
use super::engine::{RpcAdapterEngine, TlStorage};
use super::state::State;
use crate::engine::container::EngineContainer;
use crate::engine::manager::RuntimeManager;
use crate::node::Node;
use crate::state_mgr::StateManager;
use crate::transport::rdma::ops::Ops;

lazy_static! {
    pub(crate) static ref STATE_MGR: Arc<StateManager<State>> = Arc::new(StateManager::new());
}

fn create_acceptor_engine(
    runtime_manager: &RuntimeManager,
    client_pid: Pid,
    ops: Ops,
) -> Result<()> {
    let state = STATE_MGR.get_or_create_state(client_pid)?;

    // only create one cm_engine for a client process
    if state.alive_engines() > 1 {
        return Ok(());
    }

    let node = Node::new(EngineType::RpcAdapterAcceptor);
    let acceptor_engine = AcceptorEngine::new(node, state, Box::new(TlStorage { ops }));

    // always submit the engine to a dedicate runtime
    runtime_manager.submit(
        EngineContainer::new(acceptor_engine),
        SchedulingMode::Dedicate,
    );
    Ok(())
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
        const BUF_LEN: usize = 32;
        // create or get the state of the process
        let state = STATE_MGR.get_or_create_state(self.client_pid)?;
        // let salloc_state = crate::salloc::module::STATE_MGR.get_or_create_state(self.client_pid)?;
        assert_eq!(self.node.engine_type, EngineType::RpcAdapter);

        Ok(RpcAdapterEngine {
            state,
            odp_mr: None,
            tls: Box::new(TlStorage { ops: self.ops }),
            local_buffer: VecDeque::new(),
            pending_recv: 0,
            node: self.node,
            cmd_rx: self.cmd_rx,
            cmd_tx: self.cmd_tx,
            _mode: self.mode,
            indicator: None,
            recv_mr_usage: fnv::FnvHashMap::default(),
            serialization_engine: None,
            wc_read_buffer: Vec::with_capacity(BUF_LEN),
        })
    }
}

pub struct RpcAdapterModule;

impl RpcAdapterModule {
    #[allow(dead_code)]
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

        create_acceptor_engine(runtime_manager, client_pid, ops.clone())?;

        let builder = RpcAdapterEngineBuilder::new(n, client_pid, mode, cmd_rx, cmd_tx, ops);
        let engine = builder.build()?;

        Ok(engine)
    }
}
