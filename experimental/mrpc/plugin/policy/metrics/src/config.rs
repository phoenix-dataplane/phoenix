use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricsConfig {}

impl MetricsConfig {
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(config.unwrap_or(""))?;
        Ok(config)
    }
}
