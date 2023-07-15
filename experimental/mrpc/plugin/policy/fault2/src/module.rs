use anyhow::{bail, Result};
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::Fault2Engine;
use crate::config::{create_log_file, Fault2Config};

use chrono::prelude::*;
use itertools::iproduct;
use rand::Rng;

pub(crate) struct Fault2EngineBuilder {
    node: DataPathNode,
    config: Fault2Config,
}

impl Fault2EngineBuilder {
    fn new(node: DataPathNode, config: Fault2Config) -> Self {
        Fault2EngineBuilder { node, config }
    }
    // TODO! LogFile
    fn build(self) -> Result<Fault2Engine> {
        let var_probability = 0.01;

        Ok(Fault2Engine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            var_probability,
        })
    }
}

pub struct Fault2Addon {
    config: Fault2Config,
}

impl Fault2Addon {
    pub const FAULT2_ENGINE: EngineType = EngineType("Fault2Engine");
    pub const ENGINES: &'static [EngineType] = &[Fault2Addon::FAULT2_ENGINE];
}

impl Fault2Addon {
    pub fn new(config: Fault2Config) -> Self {
        Fault2Addon { config }
    }
}

impl PhoenixAddon for Fault2Addon {
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
        Fault2Addon::ENGINES
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
        if ty != Fault2Addon::FAULT2_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = Fault2EngineBuilder::new(node, self.config);
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
        if ty != Fault2Addon::FAULT2_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = Fault2Engine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
