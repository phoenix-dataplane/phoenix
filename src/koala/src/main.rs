use std::sync::Arc;

#[macro_use]
extern crate log;
use anyhow::Result;

use koala::control::Control;
use engine::manager::RuntimeManager;

fn main() -> Result<()> {
    init_env_log("KOALA_LOG", "debug");

    // create runtime manager
    let runtime_manager = Arc::new(RuntimeManager::new(1));

    // the Control now takes over
    let mut control = Control::new(runtime_manager);
    control.mainloop()
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
