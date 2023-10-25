use anyhow::{bail, Result};
use fnv::FnvHashMap as HashMap;
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::HelloAclSenderEngine;
use crate::config::HelloAclSenderConfig;
use crate::engine::AclTable;

pub(crate) struct HelloAclSenderEngineBuilder {
    node: DataPathNode,
    config: HelloAclSenderConfig,
}

impl HelloAclSenderEngineBuilder {
    fn new(node: DataPathNode, config: HelloAclSenderConfig) -> Self {
        HelloAclSenderEngineBuilder { node, config }
    }

    fn build(self) -> Result<HelloAclSenderEngine> {
        let acl_table = vec![
            AclTable {
                name: "Apple".to_string(),
                permission: "N".to_string(),
            },
            AclTable {
                name: "Banana".to_string(),
                permission: "Y".to_string(),
            },
        ];
        Ok(HelloAclSenderEngine {
            node: self.node,
            indicator: Default::default(),
            outstanding_req_pool: HashMap::default(),
            config: self.config,
            acl_table,
        })
    }
}

pub struct HelloAclSenderAddon {
    config: HelloAclSenderConfig,
}

impl HelloAclSenderAddon {
    pub const HELLO_ACL_SENDER_ENGINE: EngineType = EngineType("HelloAclSenderEngine");
    pub const ENGINES: &'static [EngineType] = &[HelloAclSenderAddon::HELLO_ACL_SENDER_ENGINE];
}

impl HelloAclSenderAddon {
    pub fn new(config: HelloAclSenderConfig) -> Self {
        HelloAclSenderAddon { config }
    }
}

impl PhoenixAddon for HelloAclSenderAddon {
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
        HelloAclSenderAddon::ENGINES
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
        if ty != HelloAclSenderAddon::HELLO_ACL_SENDER_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = HelloAclSenderEngineBuilder::new(node, self.config);
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
        if ty != HelloAclSenderAddon::HELLO_ACL_SENDER_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = HelloAclSenderEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
