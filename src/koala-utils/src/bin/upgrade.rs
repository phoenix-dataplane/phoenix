use std::env;
use std::path::{Path, PathBuf};

use uuid::Uuid;

use ipc::control;
use ipc::unix::DomainSocket;

const MAX_MSG_LEN: usize = 65536;
const DEFAULT_KOALA_PREFIX: &str = "/tmp/koala";
const DEFAULT_KOALA_CONTROL: &str = "control.sock";

fn main() {
    let uuid = Uuid::new_v4();
    let arg0 = env::args().next().unwrap();
    let appname = Path::new(&arg0).file_name().unwrap().to_string_lossy();

    let sock_path =
        PathBuf::from(DEFAULT_KOALA_PREFIX).join(format!("koala-client-{}_{}.sock", appname, uuid));

    if sock_path.exists() {
        std::fs::remove_file(&sock_path).expect("remove_file");
    }
    let mut sock = DomainSocket::bind(sock_path).unwrap();

    // let req = control::Request::Upgrade;
    // let buf = bincode::serialize(&req).unwrap();
    // assert!(buf.len() < MAX_MSG_LEN);

    // let service_path = PathBuf::from(DEFAULT_KOALA_PREFIX).join(DEFAULT_KOALA_CONTROL);
    // sock.send_to(&buf, &service_path).unwrap();
}
