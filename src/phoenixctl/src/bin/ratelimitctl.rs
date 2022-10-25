use std::env;
use std::path::{Path, PathBuf};

use structopt::StructOpt;
use uuid::Uuid;

use ipc::control::Request;
use uapi_policy_ratelimit::control_plane::Request as RateLimitRequest;
use ipc::unix::DomainSocket;

const MAX_MSG_LEN: usize = 65536;

const DEFAULT_KOALA_PREFIX: &str = "/tmp/phoenix";
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
#[structopt(name = "Koala rate limit policy control")]
struct Opts {
    #[structopt(short, long)]
    eid: u64,
    #[structopt(short, long)]
    request_per_sec: u64,
    #[structopt(short, long)]
    bucket_size: u64,
}

fn main() {
    let opts = Opts::from_args();

    let uuid = Uuid::new_v4();
    let arg0 = env::args().next().unwrap();
    let appname = Path::new(&arg0).file_name().unwrap().to_string_lossy();

    let sock_path = KOALA_PREFIX.join(format!("phoenix-client-{}_{}.sock", appname, uuid));

    if sock_path.exists() {
        std::fs::remove_file(&sock_path).expect("remove_file");
    }
    let sock = DomainSocket::bind(sock_path).unwrap();

    let request = RateLimitRequest::NewConfig(opts.request_per_sec, opts.bucket_size);
    let request_encoded = bincode::serialize(&request).unwrap();
    let req = Request::EngineRequest(opts.eid, request_encoded);
    let buf = bincode::serialize(&req).unwrap();
    assert!(buf.len() < MAX_MSG_LEN);

    let service_path = KOALA_PREFIX.join(KOALA_CONTROL_SOCK.as_path());
    sock.send_to(&buf, &service_path).unwrap();
}
