use anyhow::{bail, Result};
use fnv::FnvHashMap as HashMap;
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::MetricsServerEngine;
use crate::config::MetricsServerConfig;

pub(crate) struct MetricsServerEngineBuilder {
    node: DataPathNode,
    config: MetricsServerConfig,
}

impl MetricsServerEngineBuilder {
    fn new(node: DataPathNode, config: MetricsServerConfig) -> Self {
        MetricsServerEngineBuilder { node, config }
    }

    fn build(self) -> Result<MetricsServerEngine> {
        Ok(MetricsServerEngine {
            node: self.node,
            indicator: Default::default(),
            num_succ: 0,
            num_rej: 0,
            config: self.config,
        })
    }
}

pub struct MetricsServerAddon {
    config: MetricsServerConfig,
}

impl MetricsServerAddon {
    pub const MetricsServer_ENGINE: EngineType = EngineType("MetricsServerEngine");
    pub const ENGINES: &'static [EngineType] = &[MetricsServerAddon::MetricsServer_ENGINE];
}

impl MetricsServerAddon {
    pub fn new(config: MetricsServerConfig) -> Self {
        MetricsServerAddon { config }
    }
}

impl PhoenixAddon for MetricsServerAddon {
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
        MetricsServerAddon::ENGINES
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
        if ty != MetricsServerAddon::MetricsServer_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = MetricsServerEngineBuilder::new(node, self.config);
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
        if ty != MetricsServerAddon::MetricsServer_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = MetricsServerEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
