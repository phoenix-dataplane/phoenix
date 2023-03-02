use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SallocConfig {
    pub prefix: Option<PathBuf>,
    pub engine_basename: String,
}

impl SallocConfig {
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(config.unwrap_or(""))?;
        Ok(config)
    }
}

impl Default for SallocConfig {
    fn default() -> Self {
        SallocConfig {
            prefix: None,
            engine_basename: "salloc-engine".to_owned(),
        }
    }
}
