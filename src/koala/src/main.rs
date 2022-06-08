use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use structopt::StructOpt;

use koala::config::Config;
use koala::control::Control;
use koala::engine::manager::RuntimeManager;

pub mod logging;

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

    // init log setting from "KOALA_LOG", print messages with level lower than specified to stdout
    // print messages with level higher than KOALA_TRACING_EVENT to file
    // collect traces to tracing.json and save to output_dir.
    let _guards = logging::init_log(&config);

    // create runtime manager
    let runtime_manager = Arc::new(RuntimeManager::new(1));

    // the Control now takes over
    let mut control = Control::new(runtime_manager, config);
    control.mainloop()
}
