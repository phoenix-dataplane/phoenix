//! template file for an engine
//! we only need to change the args in config and engine constructor

use anyhow::{bail, Result};
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::LoggingServerEngine;
use crate::config::{create_log_file, LoggingServerConfig};

pub(crate) struct LoggingServerEngineBuilder {
    node: DataPathNode,
    config: LoggingServerConfig,
}

impl LoggingServerEngineBuilder {
    fn new(node: DataPathNode, config: LoggingServerConfig) -> Self {
        LoggingServerEngineBuilder { node, config }
    }

    fn build(self) -> Result<LoggingServerEngine> {
        let log_file = create_log_file();

        Ok(LoggingServerEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            log_file,
        })
    }
}

pub struct LoggingServerAddon {
    config: LoggingServerConfig,
}

impl LoggingServerAddon {
    pub const LoggingServer_ENGINE: EngineType = EngineType("LoggingServerEngine");
    pub const ENGINES: &'static [EngineType] = &[LoggingServerAddon::LoggingServer_ENGINE];
}

impl LoggingServerAddon {
    pub fn new(config: LoggingServerConfig) -> Self {
        LoggingServerAddon { config }
    }
}

impl PhoenixAddon for LoggingServerAddon {
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
        LoggingServerAddon::ENGINES
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
        if ty != LoggingServerAddon::LoggingServer_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = LoggingServerEngineBuilder::new(node, self.config);
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
        if ty != LoggingServerAddon::LoggingServer_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = LoggingServerEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
