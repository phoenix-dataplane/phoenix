use anyhow::{bail, Result};
use nix::unistd::Pid;

use phoenix::addon::{PhoenixAddon, Version};
use phoenix::engine::datapath::DataPathNode;
use phoenix::engine::{Engine, EngineType};
use phoenix::storage::ResourceCollection;

use super::engine::QosEngine;
use crate::config::QosConfig;

pub(crate) struct QosEngineBuilder {
    node: DataPathNode,
    client_pid: Pid,
    config: QosConfig,
}

impl QosEngineBuilder {
    fn new(node: DataPathNode, client_pid: Pid, config: QosConfig) -> Self {
        QosEngineBuilder {
            node,
            client_pid,
            config,
        }
    }

    fn build(self) -> Result<QosEngine> {
        Ok(QosEngine {
            node: self.node,
            indicator: Default::default(),
            client_pid: self.client_pid,
            config: self.config,
        })
    }
}

pub struct QosAddon {
    config: QosConfig,
}

impl QosAddon {
    pub const QOS_ENGINE: EngineType = EngineType("QosEngine");
    pub const ENGINES: &'static [EngineType] = &[QosAddon::QOS_ENGINE];
}

impl QosAddon {
    pub fn new(config: QosConfig) -> Self {
        QosAddon { config }
    }
}

impl PhoenixAddon for QosAddon {
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
        QosAddon::ENGINES
    }

    fn update_config(&mut self, config: &str) -> Result<()> {
        self.config = toml::from_str(config)?;
        Ok(())
    }

    fn create_engine(
        &mut self,
        ty: EngineType,
        pid: Pid,
        node: DataPathNode,
    ) -> Result<Box<dyn Engine>> {
        if ty != QosAddon::QOS_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let builder = QosEngineBuilder::new(node, pid, self.config);
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
        if ty != QosAddon::QOS_ENGINE {
            bail!("invalid engine type {:?}", ty)
        }

        let engine = QosEngine::restore(local, node, prev_version)?;
        Ok(Box::new(engine))
    }
}
