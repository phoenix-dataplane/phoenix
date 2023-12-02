use anyhow::{bail, Result};
use fnv::FnvHashMap as HashMap;
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::MutationServerEngine;
use crate::config::MutationServerConfig;

pub(crate) struct MutationServerEngineBuilder {
    node: DataPathNode,
    config: MutationServerConfig,
}

impl MutationServerEngineBuilder {
    fn new(node: DataPathNode, config: MutationServerConfig) -> Self {
        MutationServerEngineBuilder { node, config }
    }

    fn build(self) -> Result<MutationServerEngine> {
        Ok(MutationServerEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            target: "Banana".to_string(),
        })
    }
}

pub struct MutationServerAddon {
    config: MutationServerConfig,
}

impl MutationServerAddon {
    pub const MUTATION_ENGINE: EngineType = EngineType("MutationServerEngine");
    pub const ENGINES: &'static [EngineType] = &[MutationServerAddon::MUTATION_ENGINE];
}

impl MutationServerAddon {
    pub fn new(config: MutationServerConfig) -> Self {
        MutationServerAddon { config }
    }
}

impl PhoenixAddon for MutationServerAddon {
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
        MutationServerAddon::ENGINES
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
        if ty != MutationServerAddon::MUTATION_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = MutationServerEngineBuilder::new(node, self.config);
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
        if ty != MutationServerAddon::MUTATION_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = MutationServerEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
