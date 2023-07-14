use anyhow::{bail, Result};
use nix::unistd::Pid;

use phoenix_common::addon::{PhoenixAddon, Version};
use phoenix_common::engine::datapath::DataPathNode;
use phoenix_common::engine::{Engine, EngineType};
use phoenix_common::storage::ResourceCollection;

use super::engine::FileLoggingEngine;
use crate::config::{create_log_file, FileLoggingConfig};
use crate::engine::struct_rpc_events_file;

use chrono::prelude::*;
use itertools::iproduct;
use rand::Rng;

pub(crate) struct FileLoggingEngineBuilder {
    node: DataPathNode,
    config: FileLoggingConfig,
}

impl FileLoggingEngineBuilder {
    fn new(node: DataPathNode, config: FileLoggingConfig) -> Self {
        FileLoggingEngineBuilder { node, config }
    }
    // TODO! LogFile
    fn build(self) -> Result<FileLoggingEngine> {
        let mut file_rpc_events_file = create_log_file();
        Ok(FileLoggingEngine {
            node: self.node,
            indicator: Default::default(),
            config: self.config,
            file_rpc_events_file,
        })
    }
}

pub struct FileLoggingAddon {
    config: FileLoggingConfig,
}

impl FileLoggingAddon {
    pub const FILE_LOGGING_ENGINE: EngineType = EngineType("FileLoggingEngine");
    pub const ENGINES: &'static [EngineType] = &[FileLoggingAddon::FILE_LOGGING_ENGINE];
}

impl FileLoggingAddon {
    pub fn new(config: FileLoggingConfig) -> Self {
        FileLoggingAddon { config }
    }
}

impl PhoenixAddon for FileLoggingAddon {
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
        FileLoggingAddon::ENGINES
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
        if ty != FileLoggingAddon::FILE_LOGGING_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = FileLoggingEngineBuilder::new(node, self.config);
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
        if ty != FileLoggingAddon::FILE_LOGGING_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = FileLoggingEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
