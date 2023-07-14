//! template file for engine config
//! we can add custome functions here
//! so that the whole crate can import the function

use chrono::{Datelike, Timelike, Utc};
use phoenix_common::log;
use serde::{Deserialize, Serialize};

/// currently, logging engine does not need a config
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {}

impl LoggingConfig {
    /// Get config from toml file
    pub fn new(config: Option<&str>) -> anyhow::Result<Self> {
        let config = toml::from_str(config.unwrap_or(""))?;
        Ok(config)
    }
}

/// Create a log file in `/tmp/phoenix/log`
/// This function will be called every time
/// a logging engine is started or restored
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
    log::info!("create log file {}", file_name);
    let log_file = std::fs::File::create(file_name).expect("create file failed");
    log_file
}
