// $ launcher --timeout 60 --benchmark benchmark/send_bw.toml
use std::env;
use std::fs;
use std::path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;
use structopt::StructOpt;

// read from config.toml
#[derive(Debug, Clone, Deserialize)]
struct Config {
    workdir: String,
    env: toml::Value,
}

impl Config {
    pub fn from_path<P: AsRef<path::Path>>(path: P) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct WorkerSpec {
    host: String,
    bin: String,
    args: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Benchmark {
    name: String,
    description: String,
    group: String,
    worker: Vec<WorkerSpec>,
}

#[derive(Debug, StructOpt)]
#[structopt(name = "launcher", about = "Launcher of the benchmark suite.")]
struct Opt {
    /// Timeout in seconds, 0 means infinity.
    #[structopt(long = "timeout", default_value = "60")]
    timeout_secs: u64,

    /// Run a single benchmark task
    #[structopt(short, long)]
    benchmark: Option<path::PathBuf>,

    /// Run a benchmark group
    #[structopt(short, long)]
    group: Option<String>,

    /// Run with debug mode (cargo build without --release)
    #[structopt(long)]
    debug: bool,

    /// configfile
    #[structopt(short, long, default_value = "config.toml")]
    configfile: path::PathBuf,

    /// Output directory of log files
    #[structopt(short, long, default_value = "output")]
    output_dir: path::PathBuf,
}

fn open_with_create_append<P: AsRef<path::Path>>(path: P) -> fs::File {
    fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&path)
        .unwrap_or_else(|e| panic!("fail to open or create {:?}: {}", path.as_ref(), e))
}

fn get_command_str(cmd: &Command) -> String {
    let prog = cmd.get_program().to_str().unwrap();
    let args: Vec<&str> = cmd.get_args().map(|x| x.to_str().unwrap()).collect();
    let cmd_str = std::iter::once(prog)
        .chain(args)
        .collect::<Vec<_>>()
        .join(" ");
    cmd_str
}

fn start_ssh(
    opt: &Opt,
    benchmark: &Benchmark,
    worker: WorkerSpec,
    config: &Config,
    envs: &[(String, String)],
) -> impl FnOnce() -> () {
    let benchmark_name = benchmark.name.clone();
    let host = worker.host.clone();
    let output_dir = opt.output_dir.join(&benchmark_name);
    let debug_mode = opt.debug;
    let env_str = envs
        .iter()
        .map(|(name, val)| format!("{name}={val}"))
        .collect::<Vec<String>>()
        .join(" ");
    let timeout_secs = opt.timeout_secs;
    let timeout = Duration::from_secs(timeout_secs);
    let cargo_dir = config.workdir.clone();

    move || {
        let start = Instant::now();
        let (ip, port) = (&host, "22");

        let stdout_file = output_dir
            .join(format!("{}_{}.log", worker.bin, ip))
            .with_extension("stdout");
        let stderr_file = output_dir
            .join(format!("{}_{}.log", worker.bin, ip))
            .with_extension("stderr");

        let stdout = open_with_create_append(stdout_file);
        let stderr = open_with_create_append(stderr_file);
        let mut cmd = Command::new("ssh");
        cmd.stdout(stdout).stderr(stderr);
        cmd.arg("-oStrictHostKeyChecking=no")
            // .arg("-tt") // force allocating a tty
            .arg("-p")
            .arg(port)
            .arg(ip);

        // TODO(cjr): also to distribute binary program to workers
        let env_path = env::var("PATH").expect("failed to get PATH");
        if !debug_mode {
            cmd.arg(format!(
                "export PATH={} && cd {} && {} cargo run --release --bin {} -- {}",
                env_path, cargo_dir, env_str, worker.bin, worker.args
            ));
        } else {
            cmd.arg(format!(
                "export PATH={} && cd {} && {} cargo run --bin {} -- {}",
                env_path, cargo_dir, env_str, worker.bin, worker.args
            ));
        }

        // poll command status until timeout or user Ctrl-C
        let cmd_str = get_command_str(&cmd);
        log::debug!("command: {}", cmd_str);

        use std::os::unix::process::ExitStatusExt; // signal.status
        let mut child = cmd.spawn().expect("Failed to spawn {cmd_str}");
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        match status.code() {
                            Some(code) => {
                                log::error!("Exited with code: {}, cmd: {}", code, cmd_str)
                            }
                            None => log::error!(
                                "Process terminated by signal: {}, cmd: {}",
                                status.signal().unwrap(),
                                cmd_str,
                            ),
                        }
                    }
                    break;
                }
                Ok(None) => {
                    log::trace!("status not ready yet, sleep for 5 ms");
                    thread::sleep(Duration::from_millis(5));
                }
                Err(e) => {
                    panic!("Command wasn't running: {}", e);
                }
            }

            // check if kill is needed
            let outoftime = start.elapsed() > timeout;
            if TERMINATE.load(SeqCst) || outoftime {
                if outoftime {
                    log::warn!(
                        "Job has been running for {} seconds, force quitting",
                        timeout_secs
                    );
                }

                log::warn!("killing the child process: {}", cmd_str);
                // instead of SIGKILL, we use SIGTERM here to gracefully shutdown ssh process tree.
                // SIGKILL can cause terminal control characters to mess up, which must be
                // fixed later with sth like "stty sane".
                // signal::kill(nix::unistd::Pid::from_raw(child.id() as _), signal::SIGTERM)
                //     .unwrap_or_else(|e| panic!("Failed to kill: {}", e));
                child
                    .kill()
                    .unwrap_or_else(|e| panic!("Failed to kill: {}", e));
                log::warn!("child process terminated")
            }
        }
    }
}

fn run_benchmark_group(_opt: Opt, _group: String) -> anyhow::Result<()> {
    todo!();
}

fn run_benchmark(opt: Opt, path: path::PathBuf) -> anyhow::Result<()> {
    // read global config
    let config = Config::from_path(&opt.configfile)?;
    // read a single benchmark spec
    let content = fs::read_to_string(path)?;
    let spec: Benchmark = toml::from_str(&content)?;

    // create or clean output directory
    let output_dir = &opt.output_dir.join(&spec.name);
    if output_dir.exists() {
        // rm -r output_dir
        fs::remove_dir_all(output_dir)?;
    }
    fs::create_dir_all(output_dir)?;

    // process envs
    let envs: Vec<(String, String)> = if let Some(env_table) = config.env.as_table() {
        env_table
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
            .collect()
    } else {
        panic!("unexpected config envs: {:?}", config.env);
    };

    // start workers
    let mut handles = vec![];
    for w in &spec.worker {
        let h = thread::spawn(start_ssh(&opt, &spec, w.clone(), &config, &envs));
        handles.push(h);
    }

    // join
    for h in handles {
        h.join()
            .unwrap_or_else(|e| panic!("Failed to join thread: {:?}", e));
    }

    Ok(())
}

use nix::sys::signal;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::SeqCst;

static TERMINATE: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_sigint(sig: i32) {
    log::warn!("sigint catched");
    assert_eq!(sig, signal::SIGINT as i32);
    TERMINATE.store(true, SeqCst);
}

fn main() {
    init_env_log("RUST_LOG", "debug");

    let opt = Opt::from_args();
    log::info!("options: {:?}", opt);

    // register sigint handler
    let sig_action = signal::SigAction::new(
        signal::SigHandler::Handler(handle_sigint),
        signal::SaFlags::empty(),
        signal::SigSet::empty(),
    );
    unsafe { signal::sigaction(signal::SIGINT, &sig_action) }
        .expect("failed to register sighandler");

    if opt.benchmark.is_some() && opt.group.is_some() {
        log::error!("should not provide both benchmark and group, exiting.");
        return;
    }

    if let Some(b) = opt.benchmark.clone() {
        run_benchmark(opt, b).unwrap();
    } else if let Some(g) = opt.group.clone() {
        run_benchmark_group(opt, g).unwrap();
    }
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