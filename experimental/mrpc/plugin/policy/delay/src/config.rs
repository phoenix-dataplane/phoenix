use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelayConfig {
    pub delay_probability: f32,
    pub delay_ms: u64,
}

impl Default for DelayConfig {
    fn default() -> Self {
        DelayConfig {
            delay_probability: 0.2,
            delay_ms: 100,
        }
    }
}

impl DelayConfig {
    /// Get config from toml file
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(config.unwrap_or(""))?;
        Ok(config)
    }
}
