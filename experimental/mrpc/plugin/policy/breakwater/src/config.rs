use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
// BreakWaterConfig is the configuration for the BreakWater policy.
pub struct BreakWaterConfig {
    pub requests_per_sec: u64,
    pub bucket_size: u64,
    pub request_credits: u64,
    pub request_timestamp: u64,
}

// Default is the default configuration for the BreakWater policy.
impl Default for BreakWaterConfig {
    fn default() -> Self {
        BreakWaterConfig {
            requests_per_sec: 1000,
            bucket_size: 1000,
            request_credits: 100,
            request_timestamp: 10,
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
