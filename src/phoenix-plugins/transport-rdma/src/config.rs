use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RdmaTransportConfig {
    pub prefix: PathBuf,
    pub engine_basename: String,
    pub datapath_wq_depth: usize,
    pub datapath_cq_depth: usize,
    pub command_max_interval_ms: u32,
}

impl Default for RdmaTransportConfig {
    fn default() -> Self {
        RdmaTransportConfig {
            prefix: PathBuf::from("/tmp/phoenix"),
            engine_basename: String::from("transport-engine-rdma"),
            datapath_wq_depth: 32,
            datapath_cq_depth: 32,
            command_max_interval_ms: 1000,
        }
    }
}

impl RdmaTransportConfig {
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(&config.unwrap_or(""))?;
        Ok(config)
    }
}
