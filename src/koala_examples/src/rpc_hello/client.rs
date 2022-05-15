#![feature(allocator_api)]
use std::time::Instant;

use libkoala::mrpc::alloc::Vec;
use libkoala::mrpc::codegen::{GreeterClient, HelloRequest};
use libkoala::mrpc::stub::{MessageTemplate, NamedService};

use smol;

// TODO(wyj): make server addr CLI argument
const SERVER_ADDR: &str = "192.168.211.66";
const SERVER_PORT: u16 = 5000;

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    init_env_log();
    let mut client = GreeterClient::connect((SERVER_ADDR, SERVER_PORT))?;
    eprintln!("connection setup");
    smol::block_on(async {
        let mut reqs = vec![];
        for _ in 0..128 {
            let mut name = Vec::with_capacity(1000000);
            name.resize(1000000, 42);
            let msg = MessageTemplate::new(
                libkoala::mrpc::alloc::Box::new(HelloRequest { name }),
                interface::Handle(0),
                GreeterClient::FUNC_ID,
                0
            );
            reqs.push(msg);
        }
        for i in 0..8192 {
            let resp = client.say_hello(&mut reqs[0]).await.unwrap();
            eprintln!("resp {} received", i);
        }
        let start = Instant::now();
        for i in 0..8192 {
            let resp = client.say_hello(&mut reqs[0]).await.unwrap();
            // log::info!("client main got reply from mRPC engine");

            // eprintln!("resp {}: {:?}", i, resp);
            // eprintln!("resp {} received, len: {}", i, resp.name.len());
        }
        let dura = start.elapsed();
        eprintln!("dura: {:?}, speed: {:?}", dura, 8e-9 * 8192.0 * 1e6 / dura.as_secs_f64());
    });
    Ok(())
}


fn init_env_log() {
    use chrono::Utc;
    use std::io::Write;

    env_logger::Builder::default()
            .filter_level(log::LevelFilter::Info)
            .format(|buf, record| {
            let level_style = buf.default_level_style(record.level());
            writeln!(
                buf,
                "[{} {} {}:{}] {}",
                Utc::now().format("%Y-%m-%d %H:%M:%S%.6f"),
                level_style.value(record.level()),
                record.file().unwrap_or("<unnamed>"),
                record.line().unwrap_or(0),
                &record.args()
            )
        })
        .init();

    log::info!("env_logger initialized");
}
