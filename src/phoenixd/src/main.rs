use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use nix::sys::signal;

use anyhow::Result;
use clap::Parser;

use phoenix::config::Config;
use phoenix::control::Control;
use phoenix::engine::manager::RuntimeManager;

pub mod logging;

#[derive(Debug, Clone, Parser)]
#[command(name = "Phoenix Service")]
struct Opts {
    /// Phoenix config path
    #[arg(short, long, default_value = "phoenix.toml")]
    config: PathBuf,
    #[arg(long)]
    no_ansi: bool,
}

static TERMINATE: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_sigint(sig: i32) {
    assert_eq!(sig, signal::SIGINT as i32);
    TERMINATE.store(true, Ordering::Relaxed);
}

fn main() -> Result<()> {
    // load config
    let opts = Opts::parse();
    let config = Config::from_path(opts.config)?;

    // init log setting from "PHOENIX_LOG", print messages with level lower than specified to stdout
    // print messages with level higher than PHOENIX_TRACING_EVENT to file
    // collect traces to tracing.json and save to output_dir.
    let _guards = logging::init_log(&config, !opts.no_ansi);

    // create runtime manager
    let runtime_manager = Arc::new(RuntimeManager::new(&config));

    // process Ctrl-C event
    let sig_action = signal::SigAction::new(
        signal::SigHandler::Handler(handle_sigint),
        signal::SaFlags::empty(),
        signal::SigSet::empty(),
    );
    unsafe { signal::sigaction(signal::SIGINT, &sig_action) }
        .expect("failed to register sighandler");

    // the Control now takes over
    let mut control = Control::new(runtime_manager, config);
    control.mainloop(&TERMINATE)
}
