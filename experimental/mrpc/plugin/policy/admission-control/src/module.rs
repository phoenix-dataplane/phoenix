use anyhow::{bail, Result};
use fnv::FnvHashMap as HashMap;
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::AdmissionControlEngine;
use crate::config::AdmissionControlConfig;

pub(crate) struct AdmissionControlEngineBuilder {
    node: DataPathNode,
    config: AdmissionControlConfig,
}

impl AdmissionControlEngineBuilder {
    fn new(node: DataPathNode, config: AdmissionControlConfig) -> Self {
        AdmissionControlEngineBuilder { node, config }
    }

    fn build(self) -> Result<AdmissionControlEngine> {
        Ok(AdmissionControlEngine {
            node: self.node,
            indicator: Default::default(),
            total: 0,
            success: 0,
            last_ts: std::time::Instant::now(),
            config: self.config,
        })
    }
}

pub struct AdmissionControlAddon {
    config: AdmissionControlConfig,
}

impl AdmissionControlAddon {
    pub const ADMISSION_CONTROL_ENGINE: EngineType = EngineType("AdmissionControlEngine");
    pub const ENGINES: &'static [EngineType] = &[AdmissionControlAddon::ADMISSION_CONTROL_ENGINE];
}

impl AdmissionControlAddon {
    pub fn new(config: AdmissionControlConfig) -> Self {
        AdmissionControlAddon { config }
    }
}

impl PhoenixAddon for AdmissionControlAddon {
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
        AdmissionControlAddon::ENGINES
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
        if ty != AdmissionControlAddon::ADMISSION_CONTROL_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = AdmissionControlEngineBuilder::new(node, self.config);
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
        if ty != AdmissionControlAddon::ADMISSION_CONTROL_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = AdmissionControlEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
