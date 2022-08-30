use std::collections::HashMap;
use anyhow::{anyhow, bail};
use koala::addon::Version;
use koala::engine::{Engine, EnginePair, EngineType};
use koala::engine::datapath::DataPathNode;
use koala::module::{KoalaModule, ModuleCollection, ModuleDowncast, NewEngineRequest, Service, ServiceInfo};
use koala::state_mgr::SharedStateManager;
use koala::storage::{ResourceCollection, SharedStorage};
use crate::engine::SchedulerEngine;


pub struct SchedulerModule {}

impl SchedulerModule {
    pub fn new() -> Self {
        SchedulerModule {}
    }

    pub const SCHEDULER_ENGINE: EngineType = EngineType("Scheduler");
    pub const ENGINES: &'static [EngineType] = &[SchedulerModule::SCHEDULER_ENGINE];
    pub const DEPENDENCIES: &'static [EnginePair] =
        &[(SchedulerModule::SCHEDULER_ENGINE, EngineType("RpcAdapterEngine"))];

    pub const SERVICE: Service = Service("Scheduler");
}

impl KoalaModule for SchedulerModule {
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
        // let module = *self;
        let mut collections = ResourceCollection::new();
        // collections.insert("config".to_string(), Box::new(module.config));
        collections
    }

    fn migrate(&mut self, prev_module: Box<dyn KoalaModule>) {
        // till now, do nothing
        // let prev_concrete = unsafe { *prev_module.downcast_unchecked::<Self>() };
    }

    fn create_engine(&mut self, ty: EngineType, request: NewEngineRequest, shared: &mut SharedStorage, global: &mut ResourceCollection, node: DataPathNode, plugged: &ModuleCollection) -> anyhow::Result<Option<Box<dyn Engine>>> {
        if ty != SchedulerModule::SCHEDULER_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }
        if let NewEngineRequest::Auxiliary {
            pid: _,
            mode,
        } = request {
            Ok(Some(Box::new(SchedulerEngine::new(node, mode))))
        } else {
            bail!("invalid request type")
        }
    }

    fn restore_engine(&mut self, ty: EngineType, local: ResourceCollection, shared: &mut SharedStorage, global: &mut ResourceCollection, node: DataPathNode, plugged: &ModuleCollection, prev_version: Version) -> anyhow::Result<Box<dyn Engine>> {
        if ty != SchedulerModule::SCHEDULER_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }
        Ok(Box::new(SchedulerEngine::restore(local, shared, global, node, plugged, prev_version).unwrap()))
    }
}
