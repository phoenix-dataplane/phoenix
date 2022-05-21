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
    let _gurads = init_tokio_tracing(
        &config.event_env,
        &config.span_env,
        &config.default_tracing_level,
        &config.log_dir
    );

    // create runtime manager
    let runtime_manager = Arc::new(RuntimeManager::new(1));

    // the Control now takes over
    let mut control = Control::new(runtime_manager, config);
    control.mainloop()
}

fn init_tokio_tracing(event_filter_env: &str, span_filter_env: &str, default_level: &str, log_directory: &Option<String>) -> (tracing_appender::non_blocking::WorkerGuard, tracing_chrome::FlushGuard) {
    use std::str::FromStr;
    use tracing_subscriber::prelude::*;


    let format = tracing_subscriber::fmt::format()
        .with_level(true)
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .compact();

    
    let fmt_env_filter = tracing_subscriber::filter::EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::from_str(default_level).expect("invalid default tracing level").into())
        .with_env_var(event_filter_env)
        .from_env_lossy();

    
    let span_env_filter = tracing_subscriber::filter::EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::from_str(default_level).expect("invalid default tracing level").into())
        .with_env_var(span_filter_env)
        .from_env_lossy();        


    let (non_blocking, appender_guard) = if let Some(log_dir) = log_directory {
        let file_appender = tracing_appender::rolling::minutely(log_dir, "koala.log");
        tracing_appender::non_blocking(file_appender)
    } else {
        tracing_appender::non_blocking(std::io::stdout())
    };

    let fmt_layer = tracing_subscriber::fmt::layer()
        .event_format(format)
        .with_writer(non_blocking)
        .with_filter(fmt_env_filter);

    let (chrome_layer, flush_guard) = if let Some(log_dir) = log_directory {
        tracing_chrome::ChromeLayerBuilder::new()
            .file(std::path::Path::new(log_dir).join("tracing.json"))
            .trace_style(tracing_chrome::TraceStyle::Threaded)
            .build()
    }
    else {
        tracing_chrome::ChromeLayerBuilder::new()
            .trace_style(tracing_chrome::TraceStyle::Threaded)
            .build()
    };

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(chrome_layer.with_filter(span_env_filter))
        .init();

    info!("tokio_tracing initialized");

    (appender_guard, flush_guard)
}
