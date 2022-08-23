use std::env;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use uuid::Uuid;

use ipc::control::Request;
use ipc::control::{PluginDescriptor, PluginType, UpgradeRequest};
use ipc::unix::DomainSocket;

const MAX_MSG_LEN: usize = 65536;

const DEFAULT_KOALA_PREFIX: &str = "/tmp/koala";
const DEFAULT_KOALA_CONTROL: &str = "control.sock";

lazy_static::lazy_static! {
    static ref KOALA_PREFIX: PathBuf = {
        env::var("KOALA_PREFIX").map_or_else(|_| PathBuf::from(DEFAULT_KOALA_PREFIX), |p| {
            let path = PathBuf::from(p);
            assert!(path.is_dir(), "{path:?} is not a directly");
            path
        })
    };

    static ref KOALA_CONTROL_SOCK: PathBuf = {
        env::var("KOALA_CONTROL")
            .map_or_else(|_| PathBuf::from(DEFAULT_KOALA_CONTROL), PathBuf::from)
    };
}

#[derive(Debug, Clone, StructOpt)]
#[structopt(name = "Koala plugin upgrade utility")]
struct Opts {
    /// Koala config path
    #[structopt(short, long)]
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
    detach_group: Option<bool>,
}

impl Config {
    fn from_path<P: AsRef<Path>>(path: P) -> Self {
        let content = std::fs::read_to_string(path).unwrap();
        let config = toml::from_str(&content).unwrap();
        config
    }
}

fn main() {
    let opts = Opts::from_args();
    let config = Config::from_path(opts.config);

    let uuid = Uuid::new_v4();
    let arg0 = env::args().next().unwrap();
    let appname = Path::new(&arg0).file_name().unwrap().to_string_lossy();

    let sock_path = KOALA_PREFIX.join(format!("koala-client-{}_{}.sock", appname, uuid));

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
    let detach_group = config.detach_group.unwrap_or(true);

    let upgrade_request = UpgradeRequest {
        plugins,
        ty,
        flush,
        detach_group,
    };

    let req = Request::Upgrade(upgrade_request);
    let buf = bincode::serialize(&req).unwrap();
    assert!(buf.len() < MAX_MSG_LEN);

    let service_path = KOALA_PREFIX.join(KOALA_CONTROL_SOCK.as_path());
    sock.send_to(&buf, &service_path).unwrap();
}
