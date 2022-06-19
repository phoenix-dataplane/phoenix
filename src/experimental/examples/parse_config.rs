use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use interface::engine::EngineType;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    id: String,
    #[serde(alias = "type")]
    engine_type: EngineType,
    #[serde(default)]
    tx_input: Vec<String>,
    #[serde(default)]
    tx_output: Vec<String>,
    #[serde(default)]
    rx_input: Vec<String>,
    #[serde(default)]
    rx_output: Vec<String>,
}

// #[derive(Debug, Clone, Copy)]
// struct Node {
//     id: usize,
//     engine_type: EngineType,
// }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DatapathDir {
    Tx,
    Rx,
}

#[derive(Debug, Clone, Copy)]
struct Edge {
    dp: DatapathDir,
}

type Graph = petgraph::Graph<Node, Edge>;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Profile {
    log_env: String,
    default_log_level: String,
    modules: Vec<String>,
    control: Control,
    #[serde(alias = "transport-rdma")]
    transport_rdma: Option<RdmaTransportConfig>,
    node: Vec<Node>,
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

struct Config {
    toml_config: toml::Value,
    profile: Profile,
    // graph: Graph,
}

const VALID_KEYS: [&str; 4] = ["log_env", "default_log_level", "modules", "control"];

impl Config {
    fn parse_graph(config: &toml::Value, profile: &Profile) -> anyhow::Result<Graph> {
        let modules = &profile.modules;
        match config {
            toml::Value::Table(config) => {
                for key in config.keys() {
                    let engine_type_str = if let Some((a, _)) = key.split_once("-") {
                        a.to_owned()
                    } else {
                        key.clone()
                    };
                    if !modules.contains(&engine_type_str) {
                        continue;
                    }
                    // let module: Module = &config[key].to_string().parse().unwrap();
                    println!("engine_type_str: {:?}", engine_type_str);
                    println!("to_str: {:?}", config[key].to_string());
                    let module: Node = toml::from_str(&config[key].to_string()).unwrap();
                    println!("module: {:?}", module);
                }
                Err(anyhow::anyhow!("Config has a wrong format"))
            }
            _ => Err(anyhow::anyhow!("Config has a wrong format")),
        }
    }

    fn verify_config(config: &toml::Value, profile: &Profile) -> anyhow::Result<()> {
        let modules = &profile.modules;
        match config {
            toml::Value::Table(config) => {
                for key in config.keys() {
                    if VALID_KEYS.contains(&key.as_str()) {
                        continue;
                    }
                    if modules.contains(key) {
                        continue;
                    }
                    if let Some((a, _b)) = key.split_once("-") {
                        if modules.contains(&a.to_owned()) {
                            continue;
                        }
                    }
                    return Err(anyhow::anyhow!("Invalid key: {}", key));
                }
                return Ok(());
            }
            _ => Err(anyhow::anyhow!("Config has a wrong format")),
        }
    }

    fn from_path<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path).unwrap();
        let config = content.parse::<toml::Value>().unwrap();
        let profile = toml::from_str(&content).unwrap();
        // let graph = Self::parse_graph(&config, &profile)?;
        // Self::verify_config(&config, &profile)?;
        println!("profile: {:#?}", profile);
        Ok(Self {
            toml_config: config,
            profile,
            // graph,
        })
    }
}

fn main() {
    let content = fs::read_to_string("koala.toml").unwrap();
    let config = content.parse::<toml::Value>().unwrap();
    // println!("config: {:#?}", config["log_env"]);
    // println!("config: {:#?}", config["default_log_level"]);
    // println!("config: {:#?}", config["modules"]);
    // println!("config: {:#?}", config["control"]);
    // println!("config: {:#?}", config["transport-rdma"]);
    // println!("config: {:#?}", config["Mrpc-0"]);
    // if let toml::Value::Table(config) = config {
    //     for k in config.keys() {
    //         println!("key: {:?}", k);
    //     }
    // }

    let profile: Profile = toml::from_str(&content).unwrap();
    println!("profile: {:#?}", profile);

    let config = Config::from_path("koala.toml").unwrap();
}
