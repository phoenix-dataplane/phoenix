use std::env;
use std::path::{Path, PathBuf};

use interface::engine::SchedulingMode;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;
use uuid::Uuid;

use ipc::control::Request;
use ipc::control::{pid_t, AddonRequest};
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
#[structopt(name = "Koala addon manager")]
struct Opts {
    #[structopt(short, long)]
    config: PathBuf,
    #[structopt(long)]
    pid: pid_t,
    #[structopt(long)]
    gid: u64,
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

    let request = AddonRequest {
        pid: opts.pid,
        gid: opts.gid,
        addon_engine: config.addon_engine,
        tx_channels_replacements: config.tx_channels_replacements,
        rx_channels_replacements: config.rx_channels_replacements,
        group: config.group,
    };
    let req = if config.op == AddonOp::Attach {
        Request::AttachAddon(SchedulingMode::Dedicate, request)
    } else {
        Request::DetachAddon(request)
    };
    let buf = bincode::serialize(&req).unwrap();
    assert!(buf.len() < MAX_MSG_LEN);

    let service_path = KOALA_PREFIX.join(KOALA_CONTROL_SOCK.as_path());
    sock.send_to(&buf, &service_path).unwrap();
}
