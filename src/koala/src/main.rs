use std::sync::Arc;
use std::path::PathBuf;

use anyhow::Result;
use structopt::StructOpt;

#[macro_use]
extern crate tracing;

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
    let _gurad = init_tokio_tracing(&config.log_env, &config.default_log_level, &config.log_dir);

    // create runtime manager
    let runtime_manager = Arc::new(RuntimeManager::new(1));

    // the Control now takes over
    let mut control = Control::new(runtime_manager, config);
    control.mainloop()
}

fn init_tokio_tracing(filter_env: &str, default_level: &str, log_directory: &Option<String>) -> tracing_appender::non_blocking::WorkerGuard {
    use std::str::FromStr;
    

    let format = tracing_subscriber::fmt::format()
        .with_level(true)
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .compact();

    
    let env_filter = tracing_subscriber::EnvFilter::from_env(filter_env)
        .add_directive(tracing_subscriber::filter::LevelFilter::from_str(default_level).expect("invalid log level").into());

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .event_format(format);
    
    let guard = if let Some(log_dir) = log_directory {
        let file_appender = tracing_appender::rolling::minutely(log_dir, "koala.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
        subscriber.with_writer(non_blocking).init();
        guard
    } else {
        let (non_blocking, guard) = tracing_appender::non_blocking(std::io::stdout());
        subscriber.with_writer(non_blocking).init();
        guard
    };

    info!("tokio_tracing initialized");
    guard
}
