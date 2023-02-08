use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

#[macro_use]
extern crate prettytable;
use clap::Parser;
use prettytable::Table;
use uuid::Uuid;

use ipc::control::{Request, Response, ResponseKind};
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
#[command(name = "Koala service subscription viewer")]
struct Opts {
    #[arg(short, long)]
    dump: Option<PathBuf>,
}

fn main() {
    let opts = Opts::parse();

    let uuid = Uuid::new_v4();
    let arg0 = env::args().next().unwrap();
    let appname = Path::new(&arg0).file_name().unwrap().to_string_lossy();

    let sock_path = PHOENIX_PREFIX.join(format!("phoenix-client-{}_{}.sock", appname, uuid));

    if sock_path.exists() {
        std::fs::remove_file(&sock_path).expect("remove_file");
    }
    let sock = DomainSocket::bind(sock_path).unwrap();

    let req = Request::ListSubscription;
    let buf = bincode::serialize(&req).unwrap();
    assert!(buf.len() < MAX_MSG_LEN);

    let service_path = PHOENIX_PREFIX.join(PHOENIX_CONTROL_SOCK.as_path());
    sock.send_to(&buf, &service_path).unwrap();

    let mut buf = vec![0u8; 4096];
    let (_, sender) = sock.recv_from(buf.as_mut_slice()).unwrap();
    assert_eq!(sender.as_pathname(), Some(service_path.as_ref()));

    let res: Response = bincode::deserialize(&buf).unwrap();
    let kind = res.0.unwrap();
    match kind {
        ResponseKind::ListSubscription(subscriptions) => {
            if let Some(path) = opts.dump {
                let f = File::create(path).expect("unable to create file");
                let writer = BufWriter::new(f);
                serde_json::to_writer_pretty(writer, &subscriptions).unwrap();
            } else {
                let mut services = HashMap::with_capacity(subscriptions.len());
                let mut engine_tables = HashMap::new();
                for subscription in subscriptions {
                    services.insert(
                        (subscription.pid, subscription.sid),
                        (subscription.service, subscription.addons),
                    );
                    let mut table = Table::new();
                    table.add_row(row![bFc => "EngineId", "EngineType"]);
                    for (engine_id, engine_type) in subscription.engines {
                        table.add_row(row![Fc => engine_id, engine_type]);
                    }
                    engine_tables.insert((subscription.pid, subscription.sid), table);
                }

                let mut table = Table::new();
                table.add_row(row![bFm => "PID", "SID", "Service", "Addons", "Engines"]);
                for ((pid, sid), (service, addons)) in services.into_iter() {
                    let engines = engine_tables.remove(&(pid, sid)).unwrap();
                    let addons = if !addons.is_empty() {
                        addons.join(", ")
                    } else {
                        "None".to_string()
                    };
                    if engines.len() > 1 {
                        table.add_row(row![pid, sid, service, Fy->addons, Fb->engines]);
                    } else {
                        table.add_row(row![pid, sid, service, Fy->addons, Fb->"None"]);
                    }
                }
                table.printstd();
            }
        }
        _ => panic!("invalid response"),
    }
}
