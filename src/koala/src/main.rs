use std::sync::Arc;
use std::path::PathBuf;

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

    // read log config from env "KOALA_LOG", and config.log_level as the default level
    init_env_log("KOALA_LOG", &config.log_level);

    let _gurads = if config.tracing.enable {
        let guards = init_tokio_tracing(
            &config.tracing.event_level,
            &config.tracing.span_level,
            &config.tracing.output_dir,
        );
        Some(guards)
    } else {
        None
    };

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

    log::info!("env_logger initialized");
}

fn init_tokio_tracing(default_event_level: &str, default_span_level: &str, output_dir: &str) -> (tracing_appender::non_blocking::WorkerGuard, tracing_chrome::FlushGuard) {
    const EVENT_FILTER_ENV: &str = "KOALA_TRACING_EVENT";
    const SPAN_FILTER_ENV: &str = "KOALA_TRACING_SPAN";

    use std::str::FromStr;
    use tracing_subscriber::prelude::*;


    let format = tracing_subscriber::fmt::format()
        .with_level(true)
        .with_target(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .compact();

    
    let fmt_env_filter = tracing_subscriber::filter::EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::from_str(default_event_level).expect("invalid default tracing level").into())
        .with_env_var(EVENT_FILTER_ENV)
        .from_env_lossy();

    
    let span_env_filter = tracing_subscriber::filter::EnvFilter::builder()
        .with_default_directive(tracing_subscriber::filter::LevelFilter::from_str(default_span_level).expect("invalid default tracing level").into())
        .with_env_var(SPAN_FILTER_ENV)
        .from_env_lossy();        


    let (non_blocking, appender_guard) = {
        let file_appender = tracing_appender::rolling::minutely(output_dir, "event.log");
        tracing_appender::non_blocking(file_appender)
    };

    let fmt_layer = tracing_subscriber::fmt::layer()
        .event_format(format)
        .with_writer(non_blocking)
        .with_filter(fmt_env_filter);

    let (chrome_layer, flush_guard) = {
        tracing_chrome::ChromeLayerBuilder::new()
            .file(std::path::Path::new(output_dir).join("tracing.json"))
            .trace_style(tracing_chrome::TraceStyle::Threaded)
            .build()
    };

    tracing_subscriber::registry()
        .with(fmt_layer)
        .with(chrome_layer.with_filter(span_env_filter))
        .init();

    tracing::info!("tokio_tracing initialized");

    (appender_guard, flush_guard)
}
