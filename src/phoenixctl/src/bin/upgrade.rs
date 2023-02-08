use std::env;
use std::path::{Path, PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use ipc::control::Request;
use ipc::control::{PluginDescriptor, PluginType, UpgradeRequest};
use ipc::unix::DomainSocket;

const MAX_MSG_LEN: usize = 65536;

const DEFAULT_PHOENIX_PREFIX: &str = "/tmp/phoenix";
const DEFAULT_PHOENIX_CONTROL: &str = "control.sock";

lazy_static::lazy_static! {
    static ref PHOENIX_PREFIX: PathBuf = {
        env::var("PHOENIX_PREFIX").map_or_else(|_| PathBuf::from(DEFAULT_PHOENIX_PREFIX), |p| {
            let path = PathBuf::from(p);
            assert!(path.is_dir(), "{path:?} is not a directly");
            path
        })
    };

    static ref PHOENIX_CONTROL_SOCK: PathBuf = {
        env::var("PHOENIX_CONTROL")
            .map_or_else(|_| PathBuf::from(DEFAULT_PHOENIX_CONTROL), PathBuf::from)
    };
}

#[derive(Debug, Clone, Parser)]
#[command(name = "Koala plugin upgrade utility")]
struct Opts {
    /// Koala config path
    #[arg(short, long)]
    config: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default)]
    modules: Vec<PluginDescriptor>,
    #[serde(default)]
    addons: Vec<PluginDescriptor>,
    flush: Option<bool>,
    detach_subscription: Option<bool>,
}

impl Config {
    fn from_path<P: AsRef<Path>>(path: P) -> Self {
        let content = std::fs::read_to_string(path).unwrap();
        toml::from_str(&content).unwrap()
    }
}

fn main() {
    let opts = Opts::parse();
    let config = Config::from_path(opts.config);

    let uuid = Uuid::new_v4();
    let arg0 = env::args().next().unwrap();
    let appname = Path::new(&arg0).file_name().unwrap().to_string_lossy();

    let sock_path = PHOENIX_PREFIX.join(format!("phoenix-client-{}_{}.sock", appname, uuid));

    if sock_path.exists() {
        std::fs::remove_file(&sock_path).expect("remove_file");
    }
    let sock = DomainSocket::bind(sock_path).unwrap();

    assert!(
        config.modules.is_empty() ^ config.addons.is_empty(),
        "modules and addons cannot be upgraded at the same time"
    );
    let (plugins, ty) = if config.modules.is_empty() {
        (config.addons, PluginType::Addon)
    } else {
        (config.modules, PluginType::Module)
    };

    let flush = config.flush.unwrap_or(false);
    let detach_subscription = config.detach_subscription.unwrap_or(true);

    let upgrade_request = UpgradeRequest {
        plugins,
        ty,
        flush,
        detach_subscription,
    };

    let req = Request::Upgrade(upgrade_request);
    let buf = bincode::serialize(&req).unwrap();
    assert!(buf.len() < MAX_MSG_LEN);

    let service_path = PHOENIX_PREFIX.join(PHOENIX_CONTROL_SOCK.as_path());
    sock.send_to(&buf, &service_path).unwrap();
}
