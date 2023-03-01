use std::env;
use std::path::{Path, PathBuf};

use clap::Parser;
use serde::{Deserialize, Serialize};
use phoenix_api::engine::SchedulingMode;
use uuid::Uuid;

use ipc::control::Request;
use ipc::control::{pid_t, AddonRequest};
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

#[derive(Debug, Clone, clap::Parser)]
#[command(name = "Koala addon manager")]
struct Opts {
    #[arg(short, long)]
    config: PathBuf,
    #[arg(long)]
    pid: pid_t,
    #[arg(long)]
    sid: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum AddonOp {
    #[serde(alias = "attach")]
    Attach,
    #[serde(alias = "detach")]
    Detach,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    addon_engine: String,
    #[serde(default)]
    tx_channels_replacements: Vec<(String, String, usize, usize)>,
    #[serde(default)]
    rx_channels_replacements: Vec<(String, String, usize, usize)>,
    #[serde(default)]
    group: Vec<String>,
    op: AddonOp,
    config_path: Option<PathBuf>,
    config_string: Option<String>,
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

    let request = AddonRequest {
        pid: opts.pid,
        sid: opts.sid,
        addon_engine: config.addon_engine,
        tx_channels_replacements: config.tx_channels_replacements,
        rx_channels_replacements: config.rx_channels_replacements,
        group: config.group,
        config_path: config.config_path,
        config_string: config.config_string,
    };
    let req = if config.op == AddonOp::Attach {
        Request::AttachAddon(SchedulingMode::Dedicate, request)
    } else {
        Request::DetachAddon(request)
    };
    let buf = bincode::serialize(&req).unwrap();
    assert!(buf.len() < MAX_MSG_LEN);

    let service_path = PHOENIX_PREFIX.join(PHOENIX_CONTROL_SOCK.as_path());
    sock.send_to(&buf, &service_path).unwrap();
}
