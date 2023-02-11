use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{bail, Result};
use nix::unistd::Pid;
use uuid::Uuid;

use ipc::customer::ShmCustomer;
use uapi::engine::SchedulingMode;
use uapi::salloc::{cmd, dp};

use phoenix_common::engine::datapath::node::DataPathNode;
use phoenix_common::engine::{Engine, EnginePair, EngineType};
use phoenix_common::module::{
    ModuleCollection, ModuleDowncast, NewEngineRequest, PhoenixModule, Service, ServiceInfo,
    Version,
};
use phoenix_common::state_mgr::SharedStateManager;
use phoenix_common::storage::{get_default_prefix, ResourceCollection, SharedStorage};

use super::engine::SallocEngine;
use super::state::{Shared, State};
use crate::config::SallocConfig;
use crate::region::AddressMediator;

pub(crate) type CustomerType =
    ShmCustomer<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>;

pub(crate) struct SallocEngineBuilder {
    customer: CustomerType,
    _client_pid: Pid,
    _mode: SchedulingMode,
    node: DataPathNode,
    shared: Arc<Shared>,
    addr_mediator: Arc<AddressMediator>,
}

impl SallocEngineBuilder {
    fn new(
        customer: CustomerType,
        client_pid: Pid,
        mode: SchedulingMode,
        node: DataPathNode,
        shared: Arc<Shared>,
        addr_mediator: Arc<AddressMediator>,
    ) -> Self {
        SallocEngineBuilder {
            customer,
            _client_pid: client_pid,
            _mode: mode,
            node,
            shared,
            addr_mediator,
        }
    }

    fn build(self) -> Result<SallocEngine> {
        // share the state with rpc adapter
        let salloc_state = State::new(self.shared, self.addr_mediator);

        Ok(SallocEngine {
            customer: self.customer,
            indicator: Default::default(),
            node: self.node,
            state: salloc_state,
        })
    }
}

pub struct SallocModule {
    config: SallocConfig,
    pub state_mgr: SharedStateManager<Shared>,
    addr_mediator: Arc<AddressMediator>,
}

impl SallocModule {
    pub const SALLOC_ENGINE: EngineType = EngineType("SallocEngine");
    pub const ENGINES: &'static [EngineType] = &[SallocModule::SALLOC_ENGINE];

    pub const SERVICE: Service = Service("Salloc");
}

impl SallocModule {
    pub fn new(config: SallocConfig) -> Self {
        SallocModule {
            config,
            state_mgr: SharedStateManager::new(),
            addr_mediator: Arc::new(AddressMediator::new()),
        }
    }
}

impl SallocModule {
    #[inline]
    pub fn get_addr_mediator(&self) -> Arc<AddressMediator> {
        Arc::clone(&self.addr_mediator)
    }
}

impl PhoenixModule for SallocModule {
    fn service(&self) -> Option<ServiceInfo> {
        let service = ServiceInfo {
            service: SallocModule::SERVICE,
            engine: SallocModule::SALLOC_ENGINE,
            tx_channels: &[],
            rx_channels: &[],
            scheduling_groups: vec![],
        };
        Some(service)
    }

    fn engines(&self) -> &[EngineType] {
        SallocModule::ENGINES
    }

    fn dependencies(&self) -> &[EnginePair] {
        &[]
    }

    fn check_compatibility(&self, _prev: Option<&Version>, _curr: &HashMap<&str, Version>) -> bool {
        true
    }

    fn decompose(self: Box<Self>) -> ResourceCollection {
        let module = *self;
        let mut collections = ResourceCollection::new();
        collections.insert("state_mgr".to_string(), Box::new(module.state_mgr));
        collections.insert("config".to_string(), Box::new(module.config));
        collections.insert("addr_mediator".to_string(), Box::new(module.addr_mediator));
        collections
    }

    fn migrate(&mut self, prev_module: Box<dyn PhoenixModule>) {
        // NOTE(wyj): If Shared state is not upgraded, just transfer the previous module's state_mgr
        // Otherwise, there is no need to transfer.
        // Everything must be built from ground up

        // NOTE(wyj): we may better call decompose here, in case we change SallocModule type
        let prev_concrete = unsafe { *prev_module.downcast_unchecked::<Self>() };
        self.state_mgr = prev_concrete.state_mgr;
        self.addr_mediator = prev_concrete.addr_mediator;
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
        if ty != SallocModule::SALLOC_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }
        if let NewEngineRequest::Service {
            sock,
            client_path,
            mode,
            cred,
            config_string: _config_string,
        } = request
        {
            // 1. generate a path and bind a unix domain socket to it
            let uuid = Uuid::new_v4();

            // let instance_name = format!("{}-{}.sock", self.config.engine_basename, uuid);
            let instance_name = format!("{}-{}.sock", self.config.engine_basename, uuid);

            // use the phoenix_prefix if not otherwise specified
            let phoenix_prefix = get_default_prefix(global)?;
            let engine_prefix = self.config.prefix.as_ref().unwrap_or(phoenix_prefix);
            let engine_path = engine_prefix.join(instance_name);

            // 2. create customer stub
            let customer = ShmCustomer::accept(sock, client_path, mode, engine_path)?;

            // 3. the following part are expected to be done in the Engine's constructor.
            // the transport module is responsible for initializing and starting the transport engines
            let client_pid = Pid::from_raw(cred.pid.unwrap());

            let shared = self.state_mgr.get_or_create(client_pid)?;
            let builder = SallocEngineBuilder::new(
                customer,
                client_pid,
                mode,
                node,
                shared,
                Arc::clone(&self.addr_mediator),
            );

            let engine = builder.build()?;

            Ok(Some(Box::new(engine)))
        } else {
            bail!("invalid request type");
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
        if ty != SallocModule::SALLOC_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }
        let engine = SallocEngine::restore(local, shared, global, node, plugged, prev_version)?;
        Ok(Box::new(engine))
    }
}
