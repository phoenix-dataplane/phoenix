use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreakWaterConfig {
    pub requests_per_sec: u64,
    pub bucket_size: u64,
    pub request_credits: u64,
    pub request_timestamp: u64,
}

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

impl BreakWaterConfig {
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(config.unwrap_or(""))?;
        Ok(config)
    }
}
