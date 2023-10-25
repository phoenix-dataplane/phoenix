use std::path::PathBuf;

use phoenix_api_mrpc::control_plane::TransportType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MrpcLBConfig {
    /// Prefix for the control socket
    #[serde(default)]
    pub prefix: Option<PathBuf>,
    /// Base name of the control socket
    #[serde(default = "default_engine_basename")]
    pub engine_basename: String,
    /// The directory to store the build cache
    #[serde(default = "default_build_cache")]
    pub build_cache: PathBuf,
    /// Transport to use
    pub transport: TransportType,
    /// Use NIC 0 by default
    #[serde(default)]
    pub nic_index: usize,
}

impl MrpcLBConfig {
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(config.unwrap_or(""))?;
        Ok(config)
    }
}

fn default_build_cache() -> PathBuf {
    // A path relative to MrpcConfig::prefix if it's non-empty or phoenix_prefix.
    PathBuf::from("build_cache")
}

fn default_engine_basename() -> String {
    "mrpclb-engine".to_owned()
}
