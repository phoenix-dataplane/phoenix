use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TcpTransportConfig {
    pub prefix: Option<PathBuf>,
    pub engine_basename: String,
}

impl Default for TcpTransportConfig {
    fn default() -> Self {
        TcpTransportConfig {
            prefix: None,
            engine_basename: String::from("transport-engine-tcp"),
        }
    }
}

impl TcpTransportConfig {
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(config.unwrap_or(""))?;
        Ok(config)
    }
}
