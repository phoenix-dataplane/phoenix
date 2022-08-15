use std::collections::VecDeque;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use atomic::Atomic;
use minstant::Instant;
use nix::unistd::Pid;

use koala::addon::{Addon, Version};
use koala::engine::datapath::node::DataPathNode;
use koala::engine::{Engine, EngineType};
use koala::storage::ResourceCollection;

use super::engine::RateLimitEngine;
use crate::config::RateLimitConfig;

pub(crate) struct RateLimitEngineBuilder {
    node: DataPathNode,
    config: Arc<Atomic<RateLimitConfig>>,
}

impl RateLimitEngineBuilder {
    fn new(node: DataPathNode, config: Arc<Atomic<RateLimitConfig>>) -> Self {
        RateLimitEngineBuilder { node, config }
    }

    fn build(self) -> Result<RateLimitEngine> {
        eprintln!("Building rate limit engine");
        Ok(RateLimitEngine {
            node: self.node,
            indicator: Default::default(),
            config: Arc::clone(&self.config),
            last_ts: Instant::now(),
            num_tokens: 0,
            queue: VecDeque::new(),
        })
    }
}

pub struct RateLimitAddon {
    config: Arc<Atomic<RateLimitConfig>>,
}

impl RateLimitAddon {
    pub fn new(config: RateLimitConfig) -> Self {
        RateLimitAddon {
            config: Arc::new(Atomic::new(config)),
        }
    }
}

impl Addon for RateLimitAddon {
    fn check_compatibility(&self, _prev: Option<&koala::module::Version>) -> bool {
        true
    }

    fn decompose(self: Box<Self>) -> koala::storage::ResourceCollection {
        let addon = *self;
        let mut collections = ResourceCollection::new();
        collections.insert("config".to_string(), Box::new(addon.config));
        collections
    }

    #[inline]
    fn migrate(&mut self, _prev_addon: Box<dyn Addon>) {}

    fn engines(&self) -> Vec<EngineType> {
        vec![EngineType("RateLimitEngine".to_string())]
    }

    fn create_engine(
        &mut self,
        ty: &EngineType,
        _pid: Pid,
        node: DataPathNode,
    ) -> Result<Box<dyn Engine>> {
        if &ty.0 != "RateLimitEngine" {
            bail!("invalid engine type {:?}", ty)
        }

        eprintln!("DEBUG TEST 0");
        let builder = RateLimitEngineBuilder::new(node, Arc::clone(&self.config));
        let engine = builder.build()?;
        Ok(Box::new(engine))
    }

    fn restore_engine(
        &mut self,
        ty: &EngineType,
        local: ResourceCollection,
        node: DataPathNode,
        prev_version: Version,
    ) -> Result<Box<dyn Engine>> {
        if &ty.0 != "RateLimitEngine" {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = RateLimitEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
