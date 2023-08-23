use anyhow::{bail, Result};
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::LoadBalancerEngine;
use crate::config::LoadBalancerConfig;

pub(crate) struct LoadBalancerEngineBuilder {
    node: DataPathNode,
    config: LoadBalancerConfig,
}

impl LoadBalancerEngineBuilder {
    fn new(node: DataPathNode, config: LoadBalancerConfig) -> Self {
        LoadBalancerEngineBuilder { node, config }
    }

    fn build(self) -> Result<LoadBalancerEngine> {
        Ok(LoadBalancerEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
        })
    }
}

pub struct LoadBalancerAddon {
    config: LoadBalancerConfig,
}

impl LoadBalancerAddon {
    pub const LOAD_BALANCER_ENGINE: EngineType = EngineType("LoadBalancerEngine");
    pub const ENGINES: &'static [EngineType] = &[LoadBalancerAddon::LOAD_BALANCER_ENGINE];
}

impl LoadBalancerAddon {
    pub fn new(config: LoadBalancerConfig) -> Self {
        LoadBalancerAddon { config }
    }
}

impl PhoenixAddon for LoadBalancerAddon {
    fn check_compatibility(&self, _prev: Option<&Version>) -> bool {
        true
    }

    fn decompose(self: Box<Self>) -> ResourceCollection {
        let addon = *self;
        let mut collections = ResourceCollection::new();
        collections.insert("config".to_string(), Box::new(addon.config));
        collections
    }

    #[inline]
    fn migrate(&mut self, _prev_addon: Box<dyn PhoenixAddon>) {}

    fn engines(&self) -> &[EngineType] {
        LoadBalancerAddon::ENGINES
    }

    fn update_config(&mut self, config: &str) -> Result<()> {
        self.config = toml::from_str(config)?;
        Ok(())
    }

    fn create_engine(
        &mut self,
        ty: EngineType,
        _pid: Pid,
        node: DataPathNode,
    ) -> Result<Box<dyn Engine>> {
        if ty != LoadBalancerAddon::LOAD_BALANCER_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = LoadBalancerEngineBuilder::new(node, self.config);
        let engine = builder.build()?;
        Ok(Box::new(engine))
    }

    fn restore_engine(
        &mut self,
        ty: EngineType,
        local: ResourceCollection,
        node: DataPathNode,
        prev_version: Version,
    ) -> Result<Box<dyn Engine>> {
        if ty != LoadBalancerAddon::LOAD_BALANCER_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = LoadBalancerEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
