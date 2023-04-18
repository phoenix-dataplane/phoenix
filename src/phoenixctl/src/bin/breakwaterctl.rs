use std::env;
use std::path::{Path, PathBuf};

use clap::Parser;
use uuid::Uuid;

use ipc::control::Request;
use ipc::unix::DomainSocket;
use phoenix_api_policy_breakwater::control_plane::Request as BreakWaterRequest;

const MAX_MSG_LEN: usize = 65536;

const DEFAULT_PHOENIX_PREFIX: &str = "/tmp/phoenix";
const DEFAULT_PHOENIX_CONTROL: &str = "control.sock";

//We use the thread-safe lazy_static to avoid the need to initialize the static/constant variables at runtime. 
lazy_static::lazy_static! {
    static ref PHOENIX_PREFIX: PathBuf = {
        env::var("PHOENIX_PREFIX").map_or_else(|_| PathBuf::from(DEFAULT_PHOENIX_PREFIX), |p| {
            let path = PathBuf::from(p);
            assert!(path.is_dir(), "{path:?} is not a directory");
            path
        })
    };

    static ref PHOENIX_CONTROL_SOCK: PathBuf = {
        env::var("PHOENIX_CONTROL")
            .map_or_else(|_| PathBuf::from(DEFAULT_PHOENIX_CONTROL), PathBuf::from)
    };
}

//Write a struct to parse the command line arguments.

#[derive(Debug, Clone, Parser)]
#[command(name = "Phoenix breakwater policy control")]
struct Opts {
    #[arg(short, long)]
    eid: u64,
}

fn main() {
    // Parse the command line arguments
    let opts = Opts::parse();

    // Generate a random UUID
    let uuid = Uuid::new_v4();

    // Get the name of the executable
    let arg0 = env::args().next().unwrap();
    let appname = Path::new(&arg0).file_name().unwrap().to_string_lossy();

    // Create a socket path
    let sock_path = PHOENIX_PREFIX.join(format!("phoenix-client-{}_{}.sock", appname, uuid));

    // Remove the socket if it already exists
    if sock_path.exists() {
        std::fs::remove_file(&sock_path).expect("remove_file");
    }

    // Bind the socket
    let sock = DomainSocket::bind(sock_path).unwrap();

    // Initialize the request
    let request = BreakWaterRequest::NewConfig();
    
    // Serialize the request
    let request_encoded = bincode::serialize(&request).unwrap();

    // Initialize the serialized request as an EngineRequest
    let req = Request::EngineRequest(opts.eid, request_encoded);

    // Serialize the EngineRequest
    let buf = bincode::serialize(&req).unwrap();

    // Check the length of the serialized EngineRequest is less than the maximum message length and send the request
    assert!(buf.len() < MAX_MSG_LEN);

    let service_path = PHOENIX_PREFIX.join(PHOENIX_CONTROL_SOCK.as_path());
    sock.send_to(&buf, &service_path).unwrap();
}
