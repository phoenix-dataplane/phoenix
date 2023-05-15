//! template file for an engine
//! we only need to change the args in config and engine constructor

use anyhow::{bail, Result};
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::LoggingEngine;
use crate::config::{create_log_file, LoggingConfig};

pub(crate) struct LoggingEngineBuilder {
    node: DataPathNode,
    config: LoggingConfig,
}

impl LoggingEngineBuilder {
    fn new(node: DataPathNode, config: LoggingConfig) -> Self {
        LoggingEngineBuilder { node, config }
    }

    fn build(self) -> Result<LoggingEngine> {
        let log_file = create_log_file();

        Ok(LoggingEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            log_file,
        })
    }
}

pub struct LoggingAddon {
    config: LoggingConfig,
}

impl LoggingAddon {
    pub const LOGGING_ENGINE: EngineType = EngineType("LoggingEngine");
    pub const ENGINES: &'static [EngineType] = &[LoggingAddon::LOGGING_ENGINE];
}

impl LoggingAddon {
    pub fn new(config: LoggingConfig) -> Self {
        LoggingAddon { config }
    }
}

impl PhoenixAddon for LoggingAddon {
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
        LoggingAddon::ENGINES
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
        if ty != LoggingAddon::LOGGING_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = LoggingEngineBuilder::new(node, self.config);
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
        if ty != LoggingAddon::LOGGING_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = LoggingEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
