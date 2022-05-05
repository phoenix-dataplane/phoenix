use std::sync::Arc;
use std::path::PathBuf;

#[macro_use]
extern crate log;
use anyhow::Result;
use structopt::StructOpt;

use koala::config::Config;
use koala::control::Control;
use koala::engine::manager::RuntimeManager;

#[derive(Debug, Clone, StructOpt)]
#[structopt(name = "Koala Service")]
struct Opts {
    /// Koala config path
    #[structopt(short, long, default_value = "koala.toml")]
    config: PathBuf,
}

fn main() -> Result<()> {
    // load config
    let opts = Opts::from_args();
    let config = Config::from_path(opts.config)?;

    // by default, KOALA_LOG="debug"
    init_env_log(&config.log_env, &config.default_log_level);

    // create runtime manager
    let runtime_manager = Arc::new(RuntimeManager::new(1));

    // the Control now takes over
    let mut control = Control::new(runtime_manager, config);
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
