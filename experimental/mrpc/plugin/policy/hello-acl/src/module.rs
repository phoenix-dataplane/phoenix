use anyhow::{bail, Result};
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::HelloAclEngine;
use crate::config::{create_log_file, HelloAclConfig};
use crate::engine::struct_acl;

use chrono::prelude::*;
use itertools::iproduct;

pub(crate) struct HelloAclEngineBuilder {
    node: DataPathNode,
    config: HelloAclConfig,
}

impl HelloAclEngineBuilder {
    fn new(node: DataPathNode, config: HelloAclConfig) -> Self {
        HelloAclEngineBuilder { node, config }
    }
    // TODO! LogFile
    fn build(self) -> Result<HelloAclEngine> {
        let mut table_acl = Vec::new();
        table_acl.push(struct_acl {
            name: "Apple".to_string(),
            permission: "N".to_string(),
        });
        table_acl.push(struct_acl {
            name: "Banana".to_string(),
            permission: "Y".to_string(),
        });

        Ok(HelloAclEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            table_acl,
        })
    }
}

pub struct HelloAclAddon {
    config: HelloAclConfig,
}

impl HelloAclAddon {
    pub const HELLO_ACL_ENGINE: EngineType = EngineType("HelloAclEngine");
    pub const ENGINES: &'static [EngineType] = &[HelloAclAddon::HELLO_ACL_ENGINE];
}

impl HelloAclAddon {
    pub fn new(config: HelloAclConfig) -> Self {
        HelloAclAddon { config }
    }
}

impl PhoenixAddon for HelloAclAddon {
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
        HelloAclAddon::ENGINES
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
        if ty != HelloAclAddon::HELLO_ACL_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = HelloAclEngineBuilder::new(node, self.config);
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
        if ty != HelloAclAddon::HELLO_ACL_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = HelloAclEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
