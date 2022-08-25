use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use ipc::control::PluginDescriptor;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub prefix: PathBuf,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TracingConfig {
    pub enable: bool,
    pub min_event_level: String,
    pub max_event_level: String,
    pub span_level: String,
    pub output_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfilingConfig {
    pub enable_on_new_client: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub groups: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub log_level: String,
    pub log_file: Option<String>,
    pub tracing: TracingConfig,
    pub profiling: ProfilingConfig,
    pub control: Control,
    pub modules: Vec<PluginDescriptor>,
    pub addons: Vec<PluginDescriptor>,
}

impl Config {
    pub fn from_path<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        Ok(config)
    }
}
