use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QosConfig {
    // (non-strict) latency budget
    pub latency_budget_microsecs: u64,
}

impl Default for QosConfig {
    fn default() -> Self {
        QosConfig {
            latency_budget_microsecs: 10,
        }
    }
}

impl QosConfig {
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(&config.unwrap_or(""))?;
        Ok(config)
    }
}
