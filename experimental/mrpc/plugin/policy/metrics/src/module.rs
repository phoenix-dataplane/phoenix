use anyhow::{bail, Result};
use fnv::FnvHashMap as HashMap;
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::MetricsEngine;
use crate::config::MetricsConfig;

pub(crate) struct MetricsEngineBuilder {
    node: DataPathNode,
    config: MetricsConfig,
}

impl MetricsEngineBuilder {
    fn new(node: DataPathNode, config: MetricsConfig) -> Self {
        MetricsEngineBuilder { node, config }
    }

    fn build(self) -> Result<MetricsEngine> {
        Ok(MetricsEngine {
            node: self.node,
            indicator: Default::default(),
            num_succ: 0,
            num_rej: 0,
            config: self.config,
        })
    }
}

pub struct MetricsAddon {
    config: MetricsConfig,
}

impl MetricsAddon {
    pub const METRICS_ENGINE: EngineType = EngineType("MetricsEngine");
    pub const ENGINES: &'static [EngineType] = &[MetricsAddon::METRICS_ENGINE];
}

impl MetricsAddon {
    pub fn new(config: MetricsConfig) -> Self {
        MetricsAddon { config }
    }
}

impl PhoenixAddon for MetricsAddon {
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
        MetricsAddon::ENGINES
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
        if ty != MetricsAddon::METRICS_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = MetricsEngineBuilder::new(node, self.config);
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
        if ty != MetricsAddon::METRICS_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = MetricsEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
