use anyhow::{bail, Result};
use fnv::FnvHashMap as HashMap;
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::meta_pool::MetaBufferPool;
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::HelloAclReceiverEngine;
use crate::config::HelloAclReceiverConfig;

pub(crate) struct HelloAclReceiverEngineBuilder {
    node: DataPathNode,
    config: HelloAclReceiverConfig,
}

impl HelloAclReceiverEngineBuilder {
    fn new(node: DataPathNode, config: HelloAclReceiverConfig) -> Self {
        HelloAclReceiverEngineBuilder { node, config }
    }

    fn build(self) -> Result<HelloAclReceiverEngine> {
        const META_BUFFER_POOL_CAP: usize = 128;

        Ok(HelloAclReceiverEngine {
            node: self.node,
            indicator: Default::default(),
            outstanding_req_pool: HashMap::default(),
            meta_buf_pool: MetaBufferPool::new(META_BUFFER_POOL_CAP),
            config: self.config,
        })
    }
}

pub struct HelloAclReceiverAddon {
    config: HelloAclReceiverConfig,
}

impl HelloAclReceiverAddon {
    pub const HELLO_ACL_RECEIVER_ENGINE: EngineType = EngineType("HelloAclReceiverEngine");
    pub const ENGINES: &'static [EngineType] = &[HelloAclReceiverAddon::HELLO_ACL_RECEIVER_ENGINE];
}

impl HelloAclReceiverAddon {
    pub fn new(config: HelloAclReceiverConfig) -> Self {
        HelloAclReceiverAddon { config }
    }
}

impl PhoenixAddon for HelloAclReceiverAddon {
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
        HelloAclReceiverAddon::ENGINES
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
        if ty != HelloAclReceiverAddon::HELLO_ACL_RECEIVER_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = HelloAclReceiverEngineBuilder::new(node, self.config);
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
        if ty != HelloAclReceiverAddon::HELLO_ACL_RECEIVER_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = HelloAclReceiverEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
