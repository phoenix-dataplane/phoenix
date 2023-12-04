use std::collections::VecDeque;

use anyhow::{bail, Result};
use minstant::Instant;
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::RateLimitDropEngine;
use crate::config::RateLimitDropConfig;

pub(crate) struct RateLimitDropEngineBuilder {
    node: DataPathNode,
    config: RateLimitDropConfig,
}

impl RateLimitDropEngineBuilder {
    fn new(node: DataPathNode, config: RateLimitDropConfig) -> Self {
        RateLimitDropEngineBuilder { node, config }
    }

    fn build(self) -> Result<RateLimitDropEngine> {
        Ok(RateLimitDropEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            last_ts: Instant::now(),
            num_tokens: self.config.bucket_size as _,
        })
    }
}

pub struct RateLimitDropAddon {
    config: RateLimitDropConfig,
}

impl RateLimitDropAddon {
    pub const RATE_LIMIT_DROP_ENGINE: EngineType = EngineType("RateLimitDropEngine");
    pub const ENGINES: &'static [EngineType] = &[RateLimitDropAddon::RATE_LIMIT_DROP_ENGINE];
}

impl RateLimitDropAddon {
    pub fn new(config: RateLimitDropConfig) -> Self {
        RateLimitDropAddon { config }
    }
}

impl PhoenixAddon for RateLimitDropAddon {
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
        RateLimitDropAddon::ENGINES
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
        if ty != RateLimitDropAddon::RATE_LIMIT_DROP_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = RateLimitDropEngineBuilder::new(node, self.config);
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
        if ty != RateLimitDropAddon::RATE_LIMIT_DROP_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = RateLimitDropEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
