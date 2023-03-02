use anyhow::{bail, Result};
use fnv::FnvHashMap as HashMap;
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::HotelAclEngine;
use crate::config::HotelAclConfig;

pub(crate) struct HotelAclEngineBuilder {
    node: DataPathNode,
    config: HotelAclConfig,
}

impl HotelAclEngineBuilder {
    fn new(node: DataPathNode, config: HotelAclConfig) -> Self {
        HotelAclEngineBuilder { node, config }
    }

    fn build(self) -> Result<HotelAclEngine> {
        Ok(HotelAclEngine {
            node: self.node,
            indicator: Default::default(),
            outstanding_req_pool: HashMap::default(),
            config: self.config,
        })
    }
}

pub struct HotelAclAddon {
    config: HotelAclConfig,
}

impl HotelAclAddon {
    pub const HOTEL_ACL_ENGINE: EngineType = EngineType("HotelAclEngine");
    pub const ENGINES: &'static [EngineType] = &[HotelAclAddon::HOTEL_ACL_ENGINE];
}

impl HotelAclAddon {
    pub fn new(config: HotelAclConfig) -> Self {
        HotelAclAddon { config }
    }
}

impl PhoenixAddon for HotelAclAddon {
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
        HotelAclAddon::ENGINES
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
        if ty != HotelAclAddon::HOTEL_ACL_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = HotelAclEngineBuilder::new(node, self.config);
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
        if ty != HotelAclAddon::HOTEL_ACL_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = HotelAclEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
