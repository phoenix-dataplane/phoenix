use std::collections::VecDeque;

use anyhow::{bail, Result};
use minstant::Instant;
use nix::unistd::Pid;

use koala::engine::datapath::DataPathNode;
use koala::addon::{KoalaAddon, Version};
use koala::engine::{Engine, EngineType};
use koala::storage::ResourceCollection;

use super::engine::RateLimitEngine;
use crate::config::RateLimitConfig;

pub(crate) struct RateLimitEngineBuilder {
    node: DataPathNode,
    config: RateLimitConfig,
}

impl RateLimitEngineBuilder {
    fn new(node: DataPathNode, config: RateLimitConfig) -> Self {
        RateLimitEngineBuilder { node, config }
    }

    fn build(self) -> Result<RateLimitEngine> {
        Ok(RateLimitEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            last_ts: Instant::now(),
            num_tokens: 0,
            queue: VecDeque::new(),
        })
    }
}

pub struct RateLimitAddon {
    config: RateLimitConfig,
}

impl RateLimitAddon {
    pub const RATE_LIMIT_ENGINE: EngineType = EngineType("RateLimitEngine");
    pub const ENGINES: &'static [EngineType] = &[RateLimitAddon::RATE_LIMIT_ENGINE];
}
impl RateLimitAddon {
    pub fn new(config: RateLimitConfig) -> Self {
        RateLimitAddon {
            config: config, 
        }
    }
}


impl KoalaAddon for RateLimitAddon {
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
    fn migrate(&mut self, _prev_addon: Box<dyn KoalaAddon>) { }

    fn engines(&self) -> &[EngineType] {
        RateLimitAddon::ENGINES
    }

    fn create_engine(
        &mut self,
        ty: EngineType,
        _pid: Pid,
        node: DataPathNode,
    ) -> Result<Box<dyn Engine>> {
        if ty != RateLimitAddon::RATE_LIMIT_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = RateLimitEngineBuilder::new(node, self.config);
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
        if ty != RateLimitAddon::RATE_LIMIT_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = RateLimitEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
