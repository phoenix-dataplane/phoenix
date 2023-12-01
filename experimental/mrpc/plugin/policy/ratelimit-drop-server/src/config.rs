use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RateLimitDropServerConfig {
    pub requests_per_sec: u64,
    pub bucket_size: u64,
}

impl Default for RateLimitDropServerConfig {
    fn default() -> Self {
        RateLimitDropServerConfig {
            requests_per_sec: 100000,
            bucket_size: 100000,
        }
    }
}

impl RateLimitDropServerConfig {
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(config.unwrap_or(""))?;
        Ok(config)
    }
}
