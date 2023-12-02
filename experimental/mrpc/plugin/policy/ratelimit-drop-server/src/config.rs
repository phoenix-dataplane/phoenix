use chrono::{Datelike, Timelike, Utc};
use phoenix_common::log;
use serde::{Deserialize, Serialize};

use chrono::prelude::*;
use itertools::iproduct;
use rand::Rng;

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
pub fn create_log_file() -> std::fs::File {
    std::fs::create_dir_all("/tmp/phoenix/log").expect("mkdir failed");
    let now = Utc::now();
    let date_string = format!(
        "{}-{}-{}-{}-{}-{}",
        now.year(),
        now.month(),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    );
    let file_name = format!("/tmp/phoenix/log/logging_engine_{}.log", date_string);
    ///log::info!("create log file {}", file_name);
    let log_file = std::fs::File::create(file_name).expect("create file failed");
    log_file
}
