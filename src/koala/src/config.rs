use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use interface::engine::EngineType;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub(crate) id: String,
    #[serde(alias = "type")]
    pub(crate) engine_type: EngineType,
}

use std::hash::{Hash, Hasher};
impl Hash for Node {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Control {
    prefix: PathBuf,
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RdmaTransportConfig {
    prefix: PathBuf,
    engine_basename: String,
    datapath_wq_depth: usize,
    datapath_cq_depth: usize,
    command_max_interval_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub(crate) log_env: String,
    pub(crate) default_log_level: String,
    pub(crate) modules: Vec<String>,
    pub(crate) control: Control,
    #[serde(alias = "transport-rdma")]
    pub(crate) transport_rdma: Option<RdmaTransportConfig>,
    pub(crate) node: Vec<Node>,
    pub(crate) egress: Vec<Vec<String>>,
    pub(crate) ingress: Vec<Vec<String>>,
}

impl Config {
    fn from_path<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        Ok(config)
    }
}
