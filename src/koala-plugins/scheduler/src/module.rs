use std::collections::HashMap;
use koala::addon::Version;
use koala::engine::{Engine, EnginePair, EngineType};
use koala::engine::datapath::DataPathNode;
use koala::module::{KoalaModule, ModuleCollection, NewEngineRequest, Service, ServiceInfo};
use koala::storage::{ResourceCollection, SharedStorage};

pub struct SchedulerModule {}

impl SchedulerModule {
    pub fn new() -> Self {
        SchedulerModule {}
    }

    pub const SCHEDULER_ENGINE:EngineType = EngineType("Scheduler");
    pub const ENGINES:&'static [EngineType] = &[SchedulerModule::SCHEDULER_ENGINE];
    pub const DEPENDENCIES: &'static [EnginePair] =
        &[(SchedulerModule::SCHEDULER_ENGINE, EngineType("RpcAdapterEngine"))];

    pub const SERVICE: Service = Service("Scheduler");
}

impl KoalaModule for SchedulerModule{
    fn service(&self) -> Option<ServiceInfo> {
        None
    }

    fn engines(&self) -> &[EngineType] {
        Self::ENGINES
    }

    fn dependencies(&self) -> &[EnginePair] {
        Self::DEPENDENCIES
    }

    fn check_compatibility(&self, prev: Option<&Version>, curr: &HashMap<&str, Version>) -> bool {
        true
    }

    fn decompose(self: Box<Self>) -> ResourceCollection {
        todo!()
    }

    fn migrate(&mut self, prev_module: Box<dyn KoalaModule>) {
        todo!()
    }

    fn create_engine(&mut self, ty: EngineType, request: NewEngineRequest, shared: &mut SharedStorage, global: &mut ResourceCollection, node: DataPathNode, plugged: &ModuleCollection) -> anyhow::Result<Option<Box<dyn Engine>>> {
        todo!()
    }

    fn restore_engine(&mut self, ty: EngineType, local: ResourceCollection, shared: &mut SharedStorage, global: &mut ResourceCollection, node: DataPathNode, plugged: &ModuleCollection, prev_version: Version) -> anyhow::Result<Box<dyn Engine>> {
        todo!()
    }
}
