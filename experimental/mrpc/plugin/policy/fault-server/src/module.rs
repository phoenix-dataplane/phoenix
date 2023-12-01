use anyhow::{bail, Result};
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::FaultServerEngine;
use crate::config::{create_log_file, FaultServerConfig};

use chrono::prelude::*;
use itertools::iproduct;
use rand::Rng;

pub(crate) struct FaultServerEngineBuilder {
    node: DataPathNode,
    config: FaultServerConfig,
}

impl FaultServerEngineBuilder {
    fn new(node: DataPathNode, config: FaultServerConfig) -> Self {
        FaultServerEngineBuilder { node, config }
    }
    // TODO! LogFile
    fn build(self) -> Result<FaultServerEngine> {
        let var_probability = 0.01;

        Ok(FaultServerEngine {
            node: self.node,
            indicator: DeFaultServer::deFaultServer(),
            config: self.config,
            var_probability,
        })
    }
}

pub struct FaultServerAddon {
    config: FaultServerConfig,
}

impl FaultServerAddon {
    pub const FaultServer_ENGINE: EngineType = EngineType("FaultServerEngine");
    pub const ENGINES: &'static [EngineType] = &[FaultServerAddon::FaultServer_ENGINE];
}

impl FaultServerAddon {
    pub fn new(config: FaultServerConfig) -> Self {
        FaultServerAddon { config }
    }
}

impl PhoenixAddon for FaultServerAddon {
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
        FaultServerAddon::ENGINES
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
        if ty != FaultServerAddon::FaultServer_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = FaultServerEngineBuilder::new(node, self.config);
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
        if ty != FaultServerAddon::FaultServer_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = FaultServerEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
