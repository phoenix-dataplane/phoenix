use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
// BreakWaterConfig is the configuration for the BreakWater policy.
pub struct BreakWaterConfig {
}

// Default is the default configuration for the BreakWater policy.
impl Default for BreakWaterConfig {
    fn default() -> Self {
        BreakWaterConfig {
        }
    }
}

// New creates a new BreakWaterConfig from the given TOML configuration.
impl BreakWaterConfig {
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(config.unwrap_or(""))?;
        Ok(config)
    }
}
