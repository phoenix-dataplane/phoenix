use anyhow::{bail, Result};
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::FaultEngine;
use crate::config::{create_log_file, FaultConfig};

use chrono::prelude::*;
use itertools::iproduct;
use rand::Rng;

pub(crate) struct FaultEngineBuilder {
    node: DataPathNode,
    config: FaultConfig,
}

impl FaultEngineBuilder {
    fn new(node: DataPathNode, config: FaultConfig) -> Self {
        FaultEngineBuilder { node, config }
    }
    // TODO! LogFile
    fn build(self) -> Result<FaultEngine> {
        let var_probability = 0.01;

        Ok(FaultEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            var_probability,
        })
    }
}

pub struct FaultAddon {
    config: FaultConfig,
}

impl FaultAddon {
    pub const FAULT_ENGINE: EngineType = EngineType("FaultEngine");
    pub const ENGINES: &'static [EngineType] = &[FaultAddon::FAULT_ENGINE];
}

impl FaultAddon {
    pub fn new(config: FaultConfig) -> Self {
        FaultAddon { config }
    }
}

impl PhoenixAddon for FaultAddon {
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
        FaultAddon::ENGINES
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
        if ty != FaultAddon::FAULT_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = FaultEngineBuilder::new(node, self.config);
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
        if ty != FaultAddon::FAULT_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = FaultEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
