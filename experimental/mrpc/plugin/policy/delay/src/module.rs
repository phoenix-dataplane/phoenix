use anyhow::{bail, Result};
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::DelayEngine;
use crate::config::{create_log_file, DelayConfig};

use chrono::prelude::*;
use itertools::iproduct;
use rand::Rng;

pub(crate) struct DelayEngineBuilder {
    node: DataPathNode,
    config: DelayConfig,
}

impl DelayEngineBuilder {
    fn new(node: DataPathNode, config: DelayConfig) -> Self {
        DelayEngineBuilder { node, config }
    }
    // TODO! LogFile
    fn build(self) -> Result<DelayEngine> {
        let mut var_probability = 0.2;

        Ok(DelayEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            var_probability,
        })
    }
}

pub struct DelayAddon {
    config: DelayConfig,
}

impl DelayAddon {
    pub const DELAY_ENGINE: EngineType = EngineType("DelayEngine");
    pub const ENGINES: &'static [EngineType] = &[DelayAddon::DELAY_ENGINE];
}

impl DelayAddon {
    pub fn new(config: DelayConfig) -> Self {
        DelayAddon { config }
    }
}

impl PhoenixAddon for DelayAddon {
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
        DelayAddon::ENGINES
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
        if ty != DelayAddon::DELAY_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = DelayEngineBuilder::new(node, self.config);
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
        if ty != DelayAddon::DELAY_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = DelayEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
