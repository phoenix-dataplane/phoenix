use anyhow::{bail, Result};
use minstant::Instant;
use nix::unistd::Pid;

use super::engine::RateLimitDropServerEngine;
use crate::config::{create_log_file, RateLimitDropServerConfig};
use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::meta_pool::{MetaBufferPool, META_BUFFER_SIZE};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use chrono::prelude::*;
use itertools::iproduct;
use rand::Rng;

pub(crate) struct RateLimitDropServerEngineBuilder {
    node: DataPathNode,
    config: RateLimitDropServerConfig,
}

impl RateLimitDropServerEngineBuilder {
    fn new(node: DataPathNode, config: RateLimitDropServerConfig) -> Self {
        RateLimitDropServerEngineBuilder { node, config }
    }
    // TODO! LogFile
    fn build(self) -> Result<RateLimitDropServerEngine> {
        let META_BUFFER_POOL_CAP = 200;
        Ok(RateLimitDropServerEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            meta_buf_pool: MetaBufferPool::new(META_BUFFER_POOL_CAP),
            last_ts: Instant::now(),
            num_tokens: self.config.bucket_size as _,
        })
    }
}

pub struct RateLimitDropServerAddon {
    config: RateLimitDropServerConfig,
}

impl RateLimitDropServerAddon {
    pub const RATELIMIT_DROP_SERVER_ENGINE: EngineType = EngineType("RateLimitDropServerEngine");
    pub const ENGINES: &'static [EngineType] =
        &[RateLimitDropServerAddon::RATELIMIT_DROP_SERVER_ENGINE];
}

impl RateLimitDropServerAddon {
    pub fn new(config: RateLimitDropServerConfig) -> Self {
        RateLimitDropServerAddon { config }
    }
}

impl PhoenixAddon for RateLimitDropServerAddon {
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
        RateLimitDropServerAddon::ENGINES
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
        if ty != RateLimitDropServerAddon::RATELIMIT_DROP_SERVER_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = RateLimitDropServerEngineBuilder::new(node, self.config);
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
        if ty != RateLimitDropServerAddon::RATELIMIT_DROP_SERVER_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = RateLimitDropServerEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
