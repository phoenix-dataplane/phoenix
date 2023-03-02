use std::collections::{HashMap, VecDeque};
use std::os::unix::net::UCred;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use nix::unistd::Pid;
use uuid::Uuid;

use ipc::customer::ShmCustomer;
use ipc::unix::DomainSocket;
use phoenix_api::engine::SchedulingMode;
use phoenix_api::transport::tcp::{cmd, dp};

use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EnginePair, EngineType};
use phoenix_common::module::{
    ModuleCollection, ModuleDowncast, NewEngineRequest, PhoenixModule, Service, ServiceInfo,
    Version,
};
use phoenix_common::state_mgr::SharedStateManager;
use phoenix_common::storage::{get_default_prefix, ResourceCollection, SharedStorage};

use super::engine::TransportEngine;
use super::ops::Ops;
use crate::config::TcpTransportConfig;
use crate::state::{Shared, State};

pub type CustomerType =
    ShmCustomer<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>;

pub(crate) struct TransportEngineBuilder {
    customer: CustomerType,
    node: DataPathNode,
    mode: SchedulingMode,
    ops: Ops,
}

impl TransportEngineBuilder {
    fn new(customer: CustomerType, node: DataPathNode, mode: SchedulingMode, ops: Ops) -> Self {
        TransportEngineBuilder {
            customer,
            node,
            mode,
            ops,
        }
    }

    fn build(self) -> Result<TransportEngine> {
        Ok(TransportEngine {
            customer: self.customer,
            node: self.node,
            cq_err_buffer: VecDeque::new(),
            _mode: self.mode,
            ops: self.ops,
            indicator: Default::default(),
        })
    }
}

pub struct TcpTransportModule {
    config: TcpTransportConfig,
    pub state_mgr: SharedStateManager<Shared>,
}

impl TcpTransportModule {
    pub const TCP_TRANSPORT_ENGINE: EngineType = EngineType("TcpTransportEngine");
    pub const ENGINES: &'static [EngineType] = &[TcpTransportModule::TCP_TRANSPORT_ENGINE];
    pub const DEPENDENCIES: &'static [EnginePair] = &[];

    pub const SERIVCE: Service = Service("TcpTransport");
}

impl TcpTransportModule {
    pub fn new(config: TcpTransportConfig) -> Self {
        TcpTransportModule {
            config,
            state_mgr: SharedStateManager::new(),
        }
    }
}

impl PhoenixModule for TcpTransportModule {
    fn service(&self) -> Option<ServiceInfo> {
        let serivce = ServiceInfo {
            service: TcpTransportModule::SERIVCE,
            engine: TcpTransportModule::TCP_TRANSPORT_ENGINE,
            tx_channels: &[],
            rx_channels: &[],
            scheduling_groups: vec![],
        };
        Some(serivce)
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
        collections.insert("config".to_string(), Box::new(module.config));
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
        _shared: &mut SharedStorage,
        global: &mut ResourceCollection,
        node: DataPathNode,
        _plugged: &ModuleCollection,
    ) -> Result<Option<Box<dyn Engine>>> {
        match ty {
            TcpTransportModule::TCP_TRANSPORT_ENGINE => {
                if let NewEngineRequest::Service {
                    sock,
                    client_path,
                    mode,
                    cred,
                    config_string: _config_string,
                } = request
                {
                    let phoenix_prefix = get_default_prefix(global)?;
                    let engine = self.create_transport_engine(
                        sock,
                        client_path,
                        mode,
                        node,
                        phoenix_prefix,
                        cred,
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
            TcpTransportModule::TCP_TRANSPORT_ENGINE => {
                let engine =
                    TransportEngine::restore(local, shared, global, node, plugged, prev_version)?;
                Ok(Box::new(engine))
            }
            _ => bail!("invalid engine type {:?}", ty),
        }
    }
}

impl TcpTransportModule {
    fn create_transport_engine(
        &mut self,
        sock: &DomainSocket,
        client_path: &Path,
        mode: SchedulingMode,
        node: DataPathNode,
        phoenix_prefix: &PathBuf,
        cred: &UCred,
    ) -> Result<TransportEngine> {
        let uuid = Uuid::new_v4();
        let instance_name = format!("{}-{}.sock", self.config.engine_basename, uuid);

        // use the phoenix_prefix if not otherwise specified
        let engine_prefix = self.config.prefix.as_ref().unwrap_or(phoenix_prefix);
        let engine_path = engine_prefix.join(instance_name);

        let customer = ShmCustomer::accept(sock, client_path, mode, engine_path)?;

        let client_pid = Pid::from_raw(cred.pid.unwrap());
        let ops = self.create_ops(client_pid)?;

        let builder = TransportEngineBuilder::new(customer, node, mode, ops);
        let engine = builder.build()?;
        Ok(engine)
    }

    pub fn create_ops(&mut self, client_pid: Pid) -> Result<Ops> {
        let shared = self.state_mgr.get_or_create(client_pid)?;
        let state = State::new(shared);

        Ok(Ops::new(state))
    }
}
