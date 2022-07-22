use std::collections::VecDeque;
use std::os::unix::net::{SocketAddr, UCred};
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use nix::unistd::Pid;

use interface::engine::{EngineType, SchedulingMode};
use ipc;
use ipc::transport::rdma::control_plane;
use ipc::unix::DomainSocket;

use super::acceptor::engine::AcceptorEngine;
use super::engine::{RpcAdapterEngine, TlStorage};
use super::state::{Shared, State};
use crate::engine::container::EngineContainer;
use crate::engine::manager::RuntimeManager;
use crate::module::Service;
use crate::node::Node;
use crate::salloc::module::SallocModule;
use crate::salloc::state::{Shared as SallocShared, State as SallocState};
use crate::state_mgr::SharedStateManager;
use crate::transport::rdma::module::TransportModule;
use crate::transport::rdma::ops::Ops;

fn create_acceptor_engine(
    runtime_manager: &Arc<RuntimeManager>,
    state_mgr: &mut SharedStateManager<Shared>,
    client_pid: Pid,
    ops: Ops,
) -> Result<()> {
    let shared = state_mgr.get_or_create(client_pid)?;

    // only create one RpcAcceptor engine for one client process
    if Arc::strong_count(&shared) > 1 {
        return Ok(());
    }

    let state = State::new(shared);
    let node = Node::new(EngineType::RpcAdapterAcceptor);
    let acceptor_engine = AcceptorEngine::new(node, state, Box::new(TlStorage { ops }));

    // always submit the engine to a dedicate runtime
    let gid = runtime_manager.get_new_group_id(client_pid, Service(String::from("DEFAULT")));
    runtime_manager.submit(
        client_pid,
        gid,
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
    shared: Arc<Shared>,
    salloc_shared: Arc<SallocShared>,
}

impl RpcAdapterEngineBuilder {
    fn new(
        node: Node,
        client_pid: Pid,
        mode: SchedulingMode,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<ipc::mrpc::cmd::Command>,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<ipc::mrpc::cmd::Completion>,
        ops: Ops,
        shared: Arc<Shared>,
        salloc_shared: Arc<SallocShared>,
    ) -> Self {
        RpcAdapterEngineBuilder {
            node,
            client_pid,
            mode,
            cmd_rx,
            cmd_tx,
            ops,
            shared,
            salloc_shared,
        }
    }

    fn build(self) -> Result<RpcAdapterEngine> {
        let state = State::new(self.shared);
        let salloc_state = SallocState::new(self.salloc_shared);

        assert_eq!(self.node.engine_type, EngineType::RpcAdapter);

        Ok(RpcAdapterEngine {
            state,
            odp_mr: None,
            tls: Box::new(TlStorage { ops: self.ops }),
            salloc: salloc_state,
            local_buffer: VecDeque::new(),
            node: self.node,
            cmd_rx: self.cmd_rx,
            cmd_tx: self.cmd_tx,
            _mode: self.mode,
            indicator: None,
            recv_mr_usage: fnv::FnvHashMap::default(),
            serialization_engine: None,
        })
    }
}

pub struct RpcAdapterModule {
    state_mgr: SharedStateManager<Shared>,
}

impl RpcAdapterModule {
    pub fn new() -> Self {
        RpcAdapterModule {
            state_mgr: SharedStateManager::new(),
        }
    }

    #[allow(dead_code)]
    pub fn handle_request(
        &self,
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
        &mut self,
        runtime_manager: &Arc<RuntimeManager>,
        n: Node,
        mode: SchedulingMode,
        client_pid: Pid,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<ipc::mrpc::cmd::Command>,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<ipc::mrpc::cmd::Completion>,
        salloc: &mut SallocModule,
        rdam_transport: &mut TransportModule,
    ) -> Result<RpcAdapterEngine> {
        let ops = crate::transport::rdma::module::create_ops(
            runtime_manager,
            &mut rdam_transport.state_mgr,
            client_pid,
        )?;

        create_acceptor_engine(
            runtime_manager,
            &mut self.state_mgr,
            client_pid,
            ops.clone(),
        )?;

        let shared = self.state_mgr.get_or_create(client_pid)?;
        let salloc_shared = salloc.stage_mgr.get_or_create(client_pid)?;

        let builder = RpcAdapterEngineBuilder::new(
            n,
            client_pid,
            mode,
            cmd_rx,
            cmd_tx,
            ops,
            shared,
            salloc_shared,
        );
        let engine = builder.build()?;

        Ok(engine)
    }
}
