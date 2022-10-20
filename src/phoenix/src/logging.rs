use std::fmt;

use ansi_term::Colour;
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::filter::EnvFilter;
use tracing_subscriber::fmt::format::{self, FormatEvent, FormatFields};
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::registry::LookupSpan;

use phoenix::config::Config;

// The code is adapted from tokio-rs/tracing/tracing-subscriber
struct FmtLevel<'a> {
    level: &'a Level,
    ansi: bool,
}

impl<'a> FmtLevel<'a> {
    pub(crate) fn new(level: &'a Level, ansi: bool) -> Self {
        Self { level, ansi }
    }
}

const TRACE_STR: &str = "TRACE";
const DEBUG_STR: &str = "DEBUG";
const INFO_STR: &str = " INFO";
const WARN_STR: &str = " WARN";
const ERROR_STR: &str = "ERROR";

impl<'a> fmt::Display for FmtLevel<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ansi {
            match *self.level {
                Level::TRACE => write!(f, "{}", Colour::Purple.paint(TRACE_STR)),
                Level::DEBUG => write!(f, "{}", Colour::Blue.paint(DEBUG_STR)),
                Level::INFO => write!(f, "{}", Colour::Green.paint(INFO_STR)),
                Level::WARN => write!(f, "{}", Colour::Yellow.paint(WARN_STR)),
                Level::ERROR => write!(f, "{}", Colour::Red.paint(ERROR_STR)),
            }
        } else {
            match *self.level {
                Level::TRACE => f.pad(TRACE_STR),
                Level::DEBUG => f.pad(DEBUG_STR),
                Level::INFO => f.pad(INFO_STR),
                Level::WARN => f.pad(WARN_STR),
                Level::ERROR => f.pad(ERROR_STR),
            }
        }
    }
}

struct PhoenixFormatter {
    ansi: bool,
}

impl<S, N> FormatEvent<S, N> for PhoenixFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: format::Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        use chrono::Utc;
        let metadata = event.metadata();
        let fmt_level = FmtLevel::new(metadata.level(), self.ansi && writer.has_ansi_escapes());

        write!(
            writer,
            "[{} {} {}:{}] ",
            Utc::now().format("%Y-%m-%d %H:%M:%S%.6f"),
            fmt_level,
            metadata.file().unwrap_or("<unnamed>"),
            metadata.line().unwrap_or(0),
        )?;

        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

pub fn init_log(
    config: &Config,
    ansi: bool,
) -> Option<(
    tracing_appender::non_blocking::WorkerGuard,
    tracing_chrome::FlushGuard,
)> {
    use tracing_subscriber::prelude::*;

    const LOG_ENV: &str = "PHOENIX_LOG";
    let default_log_level = &config.log_level;

    let log_env_filter = EnvFilter::builder()
        .with_default_directive(
            default_log_level
                .parse()
                .expect("invalid default log level"),
        )
        .with_env_var(LOG_ENV)
        .from_env_lossy();

    let log_fmt_layer = tracing_subscriber::fmt::layer()
        .event_format(PhoenixFormatter { ansi })
        .with_filter(log_env_filter);

    let registry = tracing_subscriber::registry().with(log_fmt_layer);

    if config.tracing.enable {
        Some(init_tracing(
            &config.tracing.min_event_level,
            &config.tracing.max_event_level,
            &config.tracing.span_level,
            &config.tracing.output_dir,
            registry,
        ))
    } else {
        registry.init();
        tracing::info!("tracing-log initialized");
        None
    }
}

fn init_tracing<L>(
    default_min_event_level: &str,
    default_max_event_level: &str,
    default_span_level: &str,
    output_dir: &str,
    registry: L,
) -> (
    tracing_appender::non_blocking::WorkerGuard,
    tracing_chrome::FlushGuard,
)
where
    L: Subscriber + for<'span> LookupSpan<'span> + Send + Sync,
{
    use tracing_subscriber::prelude::*;

    const MIN_EVENT_FILTER_ENV: &str = "PHOENIX_MIN_TRACING_EVENT";
    const MAX_EVENT_FILTER_ENV: &str = "PHOENIX_MAX_TRACING_EVENT";
    const SPAN_FILTER_ENV: &str = "PHOENIX_TRACING_SPAN";

    // save events to output_dir
    let file_appender = tracing_appender::rolling::minutely(output_dir, "event.log");
    let (non_blocking, appender_guard) = tracing_appender::non_blocking(file_appender);

    // get min_event_level
    let min_tracing_env_filter = EnvFilter::builder()
        .with_default_directive(
            default_min_event_level
                .parse()
                .expect("invalid default tracing level"),
        )
        .with_env_var(MIN_EVENT_FILTER_ENV)
        .from_env_lossy();

    let min_event_level = min_tracing_env_filter
        .max_level_hint()
        .expect("{min_tracing_env_filter}")
        .into_level()
        .expect("{min_tracing_env_filter}");

    // get max_event_level
    let max_tracing_env_filter = EnvFilter::builder()
        .with_default_directive(
            default_max_event_level
                .parse()
                .expect("invalid default tracing level"),
        )
        .with_env_var(MAX_EVENT_FILTER_ENV)
        .from_env_lossy();

    let max_event_level = max_tracing_env_filter
        .max_level_hint()
        .expect("{max_tracing_env_filter}")
        .into_level()
        .expect("{max_tracing_env_filter}");

    // construct fmt layer
    let tracing_fmt_layer = tracing_subscriber::fmt::layer()
        .event_format(PhoenixFormatter { ansi: false })
        .with_writer(
            non_blocking
                .with_min_level(min_event_level)
                .with_max_level(max_event_level),
        )
        .with_filter(EnvFilter::new("trace"));

    let span_env_filter = EnvFilter::builder()
        .with_default_directive(
            default_span_level
                .parse()
                .expect("invalid default tracing level"),
        )
        .with_env_var(SPAN_FILTER_ENV)
        .from_env_lossy();

    // save spans to tracing.json in output_dir
    let (chrome_layer, flush_guard) = tracing_chrome::ChromeLayerBuilder::new()
        .file(std::path::Path::new(output_dir).join("tracing.json"))
        .trace_style(tracing_chrome::TraceStyle::Threaded)
        .build();

    registry
        .with(tracing_fmt_layer)
        .with(chrome_layer.with_filter(span_env_filter))
        .init();

    tracing::info!("tokio_tracing initialized");

    (appender_guard, flush_guard)
}
