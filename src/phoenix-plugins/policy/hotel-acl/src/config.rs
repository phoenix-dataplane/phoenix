use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HotelAclConfig {}

impl Default for HotelAclConfig {
    fn default() -> Self {
        HotelAclConfig {}
    }
}

impl HotelAclConfig {
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(&config.unwrap_or(""))?;
        Ok(config)
    }
}
