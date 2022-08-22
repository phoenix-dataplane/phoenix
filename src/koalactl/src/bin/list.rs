use std::env;
use std::path::{Path, PathBuf};

use toml::to_string_pretty;
use uuid::Uuid;

use ipc::control::{Request, Response, ResponseKind};
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

fn main() {

    let uuid = Uuid::new_v4();
    let arg0 = env::args().next().unwrap();
    let appname = Path::new(&arg0).file_name().unwrap().to_string_lossy();

    let sock_path = KOALA_PREFIX.join(format!("koala-client-{}_{}.sock", appname, uuid));

    if sock_path.exists() {
        std::fs::remove_file(&sock_path).expect("remove_file");
    }
    let sock = DomainSocket::bind(sock_path).unwrap();

    let req = Request::ListSubscription;
    let buf = bincode::serialize(&req).unwrap();
    assert!(buf.len() < MAX_MSG_LEN);

    let service_path = KOALA_PREFIX.join(KOALA_CONTROL_SOCK.as_path());
    sock.send_to(&buf, &service_path).unwrap();

    let mut buf = vec![0u8; 4096];
    let (_, sender) = sock.recv_from(buf.as_mut_slice()).unwrap();
    assert_eq!(sender.as_pathname(), Some(service_path.as_ref()));

    let res: Response = bincode::deserialize(&buf).unwrap();
    let kind = res.0.unwrap();
    match kind {
        ResponseKind::ListSubscription(subscription) => {
            println!("{}", to_string_pretty(&subscription).unwrap())
        },
        _ => panic!("invalid response"),
    }

}
