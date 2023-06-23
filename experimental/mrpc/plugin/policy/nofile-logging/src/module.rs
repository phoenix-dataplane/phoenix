use anyhow::{bail, Result};
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::NofileLoggingEngine;
use crate::config::{create_log_file, NofileLoggingConfig};

use chrono::prelude::*;
use phoenix_common::engine::datapath::RpcMessageTx;
///use itertools::iproduct;

pub(crate) struct NofileLoggingEngineBuilder {
    node: DataPathNode,
    config: NofileLoggingConfig,
}

impl NofileLoggingEngineBuilder {
    fn new(node: DataPathNode, config: NofileLoggingConfig) -> Self {
        NofileLoggingEngineBuilder { node, config }
    }
    // TODO! LogFile
    fn build(self) -> Result<NofileLoggingEngine> {
        let mut log_file = create_log_file();

        Ok(NofileLoggingEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            log_file,
        })
    }
}

pub struct NofileLoggingAddon {
    config: NofileLoggingConfig,
}

impl NofileLoggingAddon {
    pub const NOFILE_LOGGING_ENGINE: EngineType = EngineType("NofileLoggingEngine");
    pub const ENGINES: &'static [EngineType] = &[NofileLoggingAddon::NOFILE_LOGGING_ENGINE];
}

impl NofileLoggingAddon {
    pub fn new(config: NofileLoggingConfig) -> Self {
        NofileLoggingAddon { config }
    }
}

impl PhoenixAddon for NofileLoggingAddon {
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
        NofileLoggingAddon::ENGINES
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
        if ty != NofileLoggingAddon::NOFILE_LOGGING_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = NofileLoggingEngineBuilder::new(node, self.config);
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
        if ty != NofileLoggingAddon::NOFILE_LOGGING_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = NofileLoggingEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
