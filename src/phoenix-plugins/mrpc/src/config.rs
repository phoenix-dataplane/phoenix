use std::path::PathBuf;

use ipc::mrpc::control_plane::TransportType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MrpcConfig {
    pub prefix: PathBuf,
    pub engine_basename: String,
    #[serde(alias = "build_cache")]
    pub build_cache: PathBuf,
    pub transport: TransportType,
    // use NIC 0 by default
    #[serde(default)]
    pub nic_index: usize,
}

impl MrpcConfig {
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(&config.unwrap_or(""))?;
        Ok(config)
    }
}
