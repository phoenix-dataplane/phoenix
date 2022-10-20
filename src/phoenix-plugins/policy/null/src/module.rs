use anyhow::{bail, Result};
use nix::unistd::Pid;

use phoenix::addon::{PhoenixAddon, Version};
use phoenix::engine::datapath::DataPathNode;
use phoenix::engine::{Engine, EngineType};
use phoenix::storage::ResourceCollection;

use super::engine::NullEngine;
use crate::config::NullConfig;

pub(crate) struct NullEngineBuilder {
    node: DataPathNode,
    config: NullConfig,
}

impl NullEngineBuilder {
    fn new(node: DataPathNode, config: NullConfig) -> Self {
        NullEngineBuilder { node, config }
    }

    fn build(self) -> Result<NullEngine> {
        Ok(NullEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
        })
    }
}

pub struct NullAddon {
    config: NullConfig,
}

impl NullAddon {
    pub const NULL_ENGINE: EngineType = EngineType("NullEngine");
    pub const ENGINES: &'static [EngineType] = &[NullAddon::NULL_ENGINE];
}

impl NullAddon {
    pub fn new(config: NullConfig) -> Self {
        NullAddon { config }
    }
}

impl PhoenixAddon for NullAddon {
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
        NullAddon::ENGINES
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
        if ty != NullAddon::NULL_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = NullEngineBuilder::new(node, self.config);
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
        if ty != NullAddon::NULL_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = NullEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
