use anyhow::{anyhow, bail, Result};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use nix::unistd::Pid;

use uapi::engine::SchedulingMode;
use uapi_mrpc::cmd;

use phoenix_salloc::module::SallocModule;
use phoenix_salloc::region::AddressMediator;
use phoenix_salloc::state::{Shared as SallocShared, State as SallocState};
use transport_tcp::module::TcpTransportModule;
use transport_tcp::ops::Ops;

use phoenix::engine::datapath::DataPathNode;
use phoenix::engine::{Engine, EnginePair, EngineType};
use phoenix::module::{
    ModuleCollection, ModuleDowncast, NewEngineRequest, PhoenixModule, ServiceInfo, Version,
};
use phoenix::state_mgr::SharedStateManager;
use phoenix::storage::{ResourceCollection, SharedStorage};

use crate::engine::{TcpRpcAdapterEngine, TlStorage};
use crate::state::{Shared, State};

pub(crate) struct RpcAdapterEngineBuilder {
    _client_pid: Pid,
    mode: SchedulingMode,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<uapi_mrpc::cmd::Command>,
    cmd_tx: tokio::sync::mpsc::UnboundedSender<uapi_mrpc::cmd::Completion>,
    node: DataPathNode,
    ops: Ops,
    shared: Arc<Shared>,
    salloc_shared: Arc<SallocShared>,
    addr_mediator: Arc<AddressMediator>,
}

impl RpcAdapterEngineBuilder {
    #[allow(clippy::too_many_arguments)]
    fn new(
        client_pid: Pid,
        mode: SchedulingMode,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<uapi_mrpc::cmd::Completion>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<uapi_mrpc::cmd::Command>,
        node: DataPathNode,
        ops: Ops,
        shared: Arc<Shared>,
        salloc_shared: Arc<SallocShared>,
        addr_mediator: Arc<AddressMediator>,
    ) -> Self {
        RpcAdapterEngineBuilder {
            _client_pid: client_pid,
            mode,
            cmd_tx,
            cmd_rx,
            node,
            ops,
            shared,
            salloc_shared,
            addr_mediator,
        }
    }

    fn build(self) -> Result<TcpRpcAdapterEngine> {
        let state = State::new(self.shared);
        let salloc_state = SallocState::new(self.salloc_shared, self.addr_mediator);

        Ok(TcpRpcAdapterEngine {
            state,
            tls: Box::new(TlStorage { ops: self.ops }),
            local_buffer: VecDeque::new(),
            cmd_tx: self.cmd_tx,
            cmd_rx: self.cmd_rx,
            node: self.node,
            _mode: self.mode,
            indicator: Default::default(),
            recv_mr_usage: fnv::FnvHashMap::default(),
            serialization_engine: None,
            salloc: salloc_state,
            // start: std::time::Instant::now(),
            rpc_ctx: Default::default(),
        })
    }
}

pub struct TcpRpcAdapterModule {
    pub state_mgr: SharedStateManager<Shared>,
}

impl TcpRpcAdapterModule {
    pub const TCP_RPC_ADAPTER_ENGINE: EngineType = EngineType("TcpRpcAdapterEngine");
    pub const ENGINES: &'static [EngineType] = &[TcpRpcAdapterModule::TCP_RPC_ADAPTER_ENGINE];
    pub const DEPENDENCIES: &'static [EnginePair] = &[];
}

impl Default for TcpRpcAdapterModule {
    fn default() -> Self {
        Self::new()
    }
}

impl TcpRpcAdapterModule {
    pub fn new() -> Self {
        TcpRpcAdapterModule {
            state_mgr: SharedStateManager::new(),
        }
    }
}

impl PhoenixModule for TcpRpcAdapterModule {
    fn service(&self) -> Option<ServiceInfo> {
        None
    }

    fn engines(&self) -> &[EngineType] {
        Self::ENGINES
    }

    fn dependencies(&self) -> &[EnginePair] {
        Self::DEPENDENCIES
    }

    fn check_compatibility(&self, _prev: Option<&Version>, _curr: &HashMap<&str, Version>) -> bool {
        true
    }

    fn decompose(self: Box<Self>) -> ResourceCollection {
        let module = *self;
        let mut collections = ResourceCollection::new();
        collections.insert("state_mgr".to_string(), Box::new(module.state_mgr));
        collections
    }

    fn migrate(&mut self, prev_module: Box<dyn PhoenixModule>) {
        // NOTE(wyj): we may better call decompose here
        let prev_concrete = unsafe { *prev_module.downcast_unchecked::<Self>() };
        self.state_mgr = prev_concrete.state_mgr;
    }

    fn create_engine(
        &mut self,
        ty: EngineType,
        request: NewEngineRequest,
        shared: &mut SharedStorage,
        _global: &mut ResourceCollection,
        node: DataPathNode,
        plugged: &ModuleCollection,
    ) -> Result<Option<Box<dyn Engine>>> {
        let mut tcp_transport_module = plugged
            .get_mut("TcpTransport")
            .ok_or_else(|| anyhow!("fail to get TcpTransport module"))?;
        let tcp_transport = tcp_transport_module
            .downcast_mut()
            .ok_or_else(|| anyhow!("fail to downcast TcpTransport module"))?;
        let mut salloc_module = plugged
            .get_mut("Salloc")
            .ok_or_else(|| anyhow!("fail to get Salloc module"))?;
        let salloc = salloc_module
            .downcast_mut()
            .ok_or_else(|| anyhow!("fail to downcast Salloc module"))?;

        match ty {
            Self::TCP_RPC_ADAPTER_ENGINE => {
                if let NewEngineRequest::Auxiliary {
                    pid: client_pid,
                    mode,
                    config_string: _,
                } = request
                {
                    let (cmd_sender, cmd_receiver) = tokio::sync::mpsc::unbounded_channel();
                    shared
                        .command_path
                        .put_sender(Self::TCP_RPC_ADAPTER_ENGINE, cmd_sender)?;
                    let (comp_sender, comp_receiver) = tokio::sync::mpsc::unbounded_channel();
                    shared
                        .command_path
                        .put_receiver(EngineType("MrpcEngine"), comp_receiver)?;

                    let engine = self.create_rpc_adapter_engine(
                        mode,
                        client_pid,
                        comp_sender,
                        cmd_receiver,
                        node,
                        salloc,
                        tcp_transport,
                    )?;
                    Ok(Some(Box::new(engine)))
                } else {
                    bail!("invalid request type")
                }
            }
            _ => bail!("invalid engine type {:?}", ty),
        }
    }

    fn restore_engine(
        &mut self,
        ty: EngineType,
        local: ResourceCollection,
        shared: &mut SharedStorage,
        global: &mut ResourceCollection,
        node: DataPathNode,
        plugged: &ModuleCollection,
        prev_version: Version,
    ) -> Result<Box<dyn Engine>> {
        match ty {
            Self::TCP_RPC_ADAPTER_ENGINE => {
                let engine = TcpRpcAdapterEngine::restore(
                    local,
                    shared,
                    global,
                    node,
                    plugged,
                    prev_version,
                )?;
                Ok(Box::new(engine))
            }
            _ => bail!("invalid engine type {:?}", ty),
        }
    }
}

impl TcpRpcAdapterModule {
    #[allow(clippy::too_many_arguments)]
    fn create_rpc_adapter_engine(
        &mut self,
        mode: SchedulingMode,
        client_pid: Pid,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<cmd::Completion>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<cmd::Command>,
        node: DataPathNode,
        salloc: &mut SallocModule,
        tcp_transport: &mut TcpTransportModule,
    ) -> Result<TcpRpcAdapterEngine> {
        // Acceptor engine should already been created at this moment
        let ops = tcp_transport.create_ops(client_pid)?;
        // Get salloc state
        let addr_mediator = salloc.get_addr_mediator();
        let addr_mediator_clone = Arc::clone(&addr_mediator);
        let shared = self.state_mgr.get_or_create_with(client_pid, move || {
            Shared::new_from_addr_mediator(client_pid, addr_mediator_clone).unwrap()
        })?;
        let salloc_shared = salloc.state_mgr.get_or_create(client_pid)?;

        let builder = RpcAdapterEngineBuilder::new(
            client_pid,
            mode,
            cmd_tx,
            cmd_rx,
            node,
            ops,
            shared,
            salloc_shared,
            addr_mediator,
        );
        let engine = builder.build()?;
        Ok(engine)
    }
}
