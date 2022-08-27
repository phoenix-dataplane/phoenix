use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimitConfig {
    pub requests_per_sec: u64,
    pub bucket_size: u64,
}

impl RateLimitConfig {
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(&config.unwrap_or(""))?;
        Ok(config)
    }
}
