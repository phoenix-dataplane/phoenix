use anyhow::{bail, Result};
use fnv::FnvHashMap as HashMap;
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::MutationEngine;
use crate::config::MutationConfig;

pub(crate) struct MutationEngineBuilder {
    node: DataPathNode,
    config: MutationConfig,
}

impl MutationEngineBuilder {
    fn new(node: DataPathNode, config: MutationConfig) -> Self {
        MutationEngineBuilder { node, config }
    }

    fn build(self) -> Result<MutationEngine> {
        Ok(MutationEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            target: "Banana".to_string(),
        })
    }
}

pub struct MutationAddon {
    config: MutationConfig,
}

impl MutationAddon {
    pub const MUTATION_ENGINE: EngineType = EngineType("MutationEngine");
    pub const ENGINES: &'static [EngineType] = &[MutationAddon::MUTATION_ENGINE];
}

impl MutationAddon {
    pub fn new(config: MutationConfig) -> Self {
        MutationAddon { config }
    }
}

impl PhoenixAddon for MutationAddon {
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
        MutationAddon::ENGINES
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
        if ty != MutationAddon::MUTATION_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = MutationEngineBuilder::new(node, self.config);
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
        if ty != MutationAddon::MUTATION_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = MutationEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
