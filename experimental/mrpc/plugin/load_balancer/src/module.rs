use anyhow::{anyhow, bail, Result};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use nix::unistd::Pid;

use phoenix_api::engine::SchedulingMode;
use phoenix_api_mrpc::cmd;

use phoenix_salloc::module::SallocModule;
use phoenix_salloc::region::AddressMediator;
use phoenix_salloc::state::{Shared as SallocShared, State as SallocState};
use transport_tcp::module::TcpTransportModule;
use transport_tcp::ops::Ops;

use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EnginePair, EngineType};
use phoenix_common::module::{
    ModuleCollection, ModuleDowncast, NewEngineRequest, PhoenixModule, ServiceInfo, Version,
};
use phoenix_common::state_mgr::SharedStateManager;
use phoenix_common::storage::{ResourceCollection, SharedStorage};

use crate::engine::{LoadBalancerEngine, TlStorage};

pub(crate) struct LoadBalancerEngineBuilder {
    _client_pid: Pid,
    mode: SchedulingMode,
    cmd_rx_upstream: tokio::sync::mpsc::UnboundedReceiver<phoenix_api_mrpc::cmd::Command>,
    cmd_tx_upstream: tokio::sync::mpsc::UnboundedSender<phoenix_api_mrpc::cmd::Completion>,
    cmd_rx_downstream: tokio::sync::mpsc::UnboundedReceiver<phoenix_api_mrpc::cmd::Completion>,
    cmd_tx_downstream: tokio::sync::mpsc::UnboundedSender<phoenix_api_mrpc::cmd::Command>,
    node: DataPathNode,
}

impl LoadBalancerEngineBuilder {
    #[allow(clippy::too_many_arguments)]
    fn new(
        client_pid: Pid,
        mode: SchedulingMode,
        cmd_tx_upstream: tokio::sync::mpsc::UnboundedSender<phoenix_api_mrpc::cmd::Completion>,
        cmd_rx_upstream: tokio::sync::mpsc::UnboundedReceiver<phoenix_api_mrpc::cmd::Command>,
        cmd_tx_downstream: tokio::sync::mpsc::UnboundedSender<phoenix_api_mrpc::cmd::Command>,
        cmd_rx_downstream: tokio::sync::mpsc::UnboundedReceiver<phoenix_api_mrpc::cmd::Completion>,

        node: DataPathNode,
    ) -> Self {
        LoadBalancerEngineBuilder {
            _client_pid: client_pid,
            mode,
            cmd_tx_upstream,
            cmd_rx_upstream,
            cmd_tx_downstream,
            cmd_rx_downstream,
            node,
        }
    }

    fn build(self) -> Result<LoadBalancerEngine> {
        Ok(LoadBalancerEngine {
            p2v: Default::default(),
            v2p: Default::default(),
            buffer: Default::default(),
            cmd_tx_upstream: self.cmd_tx_upstream,
            cmd_rx_upstream: self.cmd_rx_upstream,
            cmd_tx_downstream: self.cmd_tx_downstream,
            cmd_rx_downstream: self.cmd_rx_downstream,
            node: self.node,
            _mode: self.mode,
            indicator: Default::default(),
            // start: std::time::Instant::now(),
            rpc_ctx: Default::default(),
        })
    }
}

pub struct LoadBalancerModule {}

impl LoadBalancerModule {
    pub const LOAD_BALANCER_ENGINE: EngineType = EngineType("LoadBalancerEngine");
    pub const ENGINES: &'static [EngineType] = &[LoadBalancerModule::LOAD_BALANCER_ENGINE];
    pub const DEPENDENCIES: &'static [EnginePair] = &[];
}

impl Default for LoadBalancerModule {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadBalancerModule {
    pub fn new() -> Self {
        LoadBalancerModule {}
    }
}

impl PhoenixModule for LoadBalancerModule {
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
        collections
    }

    fn migrate(&mut self, prev_module: Box<dyn PhoenixModule>) {
        // NOTE(wyj): we may better call decompose here
        let prev_concrete = unsafe { *prev_module.downcast_unchecked::<Self>() };
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
        match ty {
            Self::LOAD_BALANCER_ENGINE => {
                if let NewEngineRequest::Auxiliary {
                    pid: client_pid,
                    mode,
                    config_string: _,
                } = request
                {
                    let (cmd_sender, cmd_receiver) = tokio::sync::mpsc::unbounded_channel();

                    shared
                        .command_path
                        .put_sender(Self::LOAD_BALANCER_ENGINE, cmd_sender)?;

                    let (comp_sender, comp_receiver) = tokio::sync::mpsc::unbounded_channel();

                    shared
                        .command_path
                        .put_receiver(Self::LOAD_BALANCER_ENGINE, comp_receiver)?;

                    let tx_down = shared
                        .command_path
                        .get_sender(&EngineType("TcpRpcAdapterEngine"))?;

                    let rx_down = shared
                        .command_path
                        .get_receiver(&EngineType("TcpRpcAdapterEngine"))?;

                    let engine = self.create_load_balancer_engine(
                        mode,
                        client_pid,
                        comp_sender,
                        cmd_receiver,
                        tx_down,
                        rx_down,
                        node,
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
            Self::LOAD_BALANCER_ENGINE => {
                let engine = LoadBalancerEngine::restore(
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

impl LoadBalancerModule {
    #[allow(clippy::too_many_arguments)]
    fn create_load_balancer_engine(
        &mut self,
        mode: SchedulingMode,
        client_pid: Pid,
        cmd_tx_upstream: tokio::sync::mpsc::UnboundedSender<cmd::Completion>,
        cmd_rx_upstream: tokio::sync::mpsc::UnboundedReceiver<cmd::Command>,
        cmd_tx_downstream: tokio::sync::mpsc::UnboundedSender<cmd::Command>,
        cmd_rx_downstream: tokio::sync::mpsc::UnboundedReceiver<cmd::Completion>,
        node: DataPathNode,
    ) -> Result<LoadBalancerEngine> {
        // Acceptor engine should already been created at this momen

        let builder = LoadBalancerEngineBuilder::new(
            client_pid,
            mode,
            cmd_tx_upstream,
            cmd_rx_upstream,
            cmd_tx_downstream,
            cmd_rx_downstream,
            node,
        );
        let engine = builder.build()?;
        Ok(engine)
    }
}
