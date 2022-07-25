use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MrpcConfig {
    pub prefix: PathBuf,
    pub engine_basename: String,
    #[serde(alias = "build_cache")]
    pub build_cache: PathBuf,
}

impl Default for MrpcConfig {
    fn default() -> Self {
        MrpcConfig {
            prefix: PathBuf::from("/tmp/koala"),
            engine_basename: String::from("mrpc-engine"),
            build_cache: PathBuf::from("/tmp/koala/build-cache"),
        }
    }
}

impl MrpcConfig {
    pub(crate) fn from_path<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        Ok(config)
    }
}
