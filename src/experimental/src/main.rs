#[macro_use]
extern crate log;
use anyhow::Result;

use crossbeam::thread;

use experimental::module::Module;
use experimental::transport::module::TransportModule;

fn main() -> Result<()> {
    init_env_log("KOALA_LOG", "debug");

    // start transport module

    thread::scope(|s| {
        let handle = s.spawn(|_| {
            TransportModule::new().bootstrap().unwrap();
        });

        handle.join().unwrap();
    })
    .unwrap();

    Ok(())
}

fn init_env_log(filter_env: &str, default_level: &str) {
    use chrono::Utc;
    use std::io::Write;

    let env = env_logger::Env::new().filter_or(filter_env, default_level);
    env_logger::Builder::from_env(env)
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

    info!("env_logger initialized");
}
