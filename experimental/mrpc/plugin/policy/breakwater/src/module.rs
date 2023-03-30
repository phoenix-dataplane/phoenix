use std::collections::VecDeque;

use anyhow::{bail, Result};
use minstant::Instant;
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::BreakWaterEngine;
use crate::config::BreakWaterConfig;

pub(crate) struct BreakWaterEngineBuilder {
    node: DataPathNode,
    config: BreakWaterConfig,
}

impl BreakWaterEngineBuilder {
    fn new(node: DataPathNode, config: BreakWaterConfig) -> Self {
        BreakWaterEngineBuilder { node, config }
    }

    fn build(self) -> Result<BreakWaterEngine> {
        Ok(BreakWaterEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            last_ts: Instant::now(),
            num_tokens: self.config.bucket_size as _,
            queue: VecDeque::new(),
        })
    }
}

pub struct BreakWaterAddon {
    config: BreakWaterConfig,
}

impl BreakWaterAddon {
    pub const BREAK_WATER_ENGINE: EngineType = EngineType("BreakWaterEngine");
    pub const ENGINES: &'static [EngineType] = &[BreakWaterAddon::BREAK_WATER_ENGINE];
}

impl BreakWaterAddon {
    pub fn new(config: BreakWaterConfig) -> Self {
        BreakWaterAddon { config }
    }
}

impl PhoenixAddon for BreakWaterAddon {
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
        BreakWaterAddon::ENGINES
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
        if ty != BreakWaterAddon::BREAK_WATER_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = BreakWaterEngineBuilder::new(node, self.config);
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
        if ty != BreakWaterAddon::BREAK_WATER_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = BreakWaterEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
