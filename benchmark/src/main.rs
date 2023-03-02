// $ launcher --timeout 60 --benchmark benchmark/send_bw.toml
use std::env;
use std::fs;
use std::io;
use std::os::unix::io::AsRawFd;
use std::path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use ansi_term::Colour::Green;
use serde::Deserialize;
use structopt::StructOpt;

pub mod line_reader;
use crate::line_reader::{LineReader, NonBlockingLineReader};

pub mod tee;

// read from config.toml
#[derive(Debug, Clone, Deserialize)]
struct Config {
    workdir: path::PathBuf,
    env: toml::Value,
}

impl Config {
    pub fn from_path<P: AsRef<path::Path>>(path: P) -> anyhow::Result<Self> {
        let content = fs::read_to_string(path)?;
        let config = toml::from_str(&content)?;
        Ok(config)
    }
}

const fn default_term_signal() -> usize {
    15
}

const fn default_start_delay() -> u64 {
    1
}

#[derive(Debug, Clone, Deserialize)]
struct WorkerSpec {
    host: String,
    bin: String,
    args: String,
    #[serde(default)]
    dependencies: Vec<usize>,
    #[serde(default = "default_term_signal", rename = "term")]
    term_signal: usize,
}

#[derive(Debug, Clone, Deserialize)]
struct Benchmark {
    name: String,
    description: String,
    group: String,
    timeout_secs: Option<u64>,
    #[serde(default = "default_start_delay")]
    start_delay: u64,
    worker: Vec<WorkerSpec>,
}

#[derive(Debug, StructOpt)]
#[structopt(name = "launcher", about = "Launcher of the benchmark suite.")]
struct Opt {
    /// Timeout in seconds, 0 means infinity. Can be overwritten by specific case configs.
    #[structopt(long = "timeout", default_value = "60")]
    global_timeout_secs: u64,

    /// Run a single benchmark task
    #[structopt(short, long)]
    benchmark: path::PathBuf,

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
    #[structopt(short, long)]
    output_dir: Option<path::PathBuf>,

    /// Do out print to stdout
    #[structopt(short, long)]
    silent: bool,

    /// Dry-run. Use this option to check the configs
    #[structopt(long)]
    dry_run: bool,

    /// kill all threads if any thread ends
    #[structopt(long)]
    logical_and: bool,
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
    std::iter::once(prog)
        .chain(args)
        .collect::<Vec<_>>()
        .join(" ")
}

fn create_non_blocking_reader<R: io::Read + AsRawFd + 'static, W: io::Write + 'static>(
    r: R,
    writer: Option<W>,
) -> Box<dyn NonBlockingLineReader> {
    if let Some(w) = writer {
        Box::new(LineReader::new(tee::TeeReader::new(r, w)))
    } else {
        Box::new(LineReader::new(tee::TeeReader::new(r, tee::DevNull)))
    }
}

fn wait_command(
    mut cmd: Command,
    mut kill_cmd: Option<Command>,
    timeout: Duration,
    host: &str,
    stdout_writer: Option<fs::File>,
    stderr_writer: Option<fs::File>,
    logical_and: bool,
) -> io::Result<()> {
    let start = Instant::now();
    let cmd_str = get_command_str(&cmd);

    use std::os::unix::process::ExitStatusExt; // signal.status
    let mut child = cmd
        .spawn()
        .unwrap_or_else(|e| panic!("Failed to spawn '{cmd_str}' because: {e}"));

    let mut stdout_reader = child
        .stdout
        .take()
        .map(|r| create_non_blocking_reader(r, stdout_writer));
    let mut stderr_reader = child
        .stderr
        .take()
        .map(|r| create_non_blocking_reader(r, stderr_writer));

    loop {
        if let Some(reader) = stdout_reader.as_mut() {
            while let Some(line) = reader.next_line()? {
                println!("[{}] {}", host, std::str::from_utf8(&line).unwrap());
            }
        }
        if let Some(reader) = stderr_reader.as_mut() {
            while let Some(line) = reader.next_line()? {
                println!("[{}] {}", host, std::str::from_utf8(&line).unwrap());
            }
        }

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
                if stdout_reader.is_none() && stdout_reader.is_none() {
                    log::trace!("status not ready yet, sleep for 5 ms");
                    thread::sleep(Duration::from_millis(5));
                }
            }
            Err(e) => {
                panic!("Command wasn't running: {}", e);
            }
        }

        // check if kill is needed
        let outoftime = start.elapsed() > timeout;
        let terminate_ts = (*TERMINATE.lock().unwrap()).unwrap_or(start);
        if terminate_ts > start || outoftime {
            if outoftime {
                log::warn!("Job has been running for {:?}, force quitting", timeout);
            }

            log::warn!("killing the child process: {}", cmd_str);
            // instead of SIGKILL, we use SIGTERM here to gracefully shutdown ssh process tree.
            // SIGKILL can cause terminal control characters to mess up, which must be
            // fixed later with sth like "stty sane".
            signal::kill(nix::unistd::Pid::from_raw(child.id() as _), signal::SIGTERM)
                .unwrap_or_else(|e| panic!("Failed to kill: {}", e));
            // child
            //     .kill()
            //     .unwrap_or_else(|e| panic!("Failed to kill: {}", e));
            log::warn!("child process terminated");

            thread::sleep(Duration::from_millis(1000));

            if let Some(kill_cmd) = kill_cmd.take() {
                wait_command(
                    kill_cmd,
                    None,
                    Duration::from_secs(2),
                    "",
                    None,
                    None,
                    false,
                )?;
            }
        }
    }

    if logical_and {
        *TERMINATE.lock().unwrap() = Some(Instant::now());
    }

    Ok(())
}

fn start_ssh(
    opt: &Opt,
    benchmark: &Benchmark,
    worker: WorkerSpec,
    config: &Config,
    envs: &[(String, String)],
    delay: Duration,
) -> impl FnOnce() {
    let benchmark_name = benchmark.name.clone();
    let host = worker.host.clone();
    let output_dir = opt.output_dir.as_ref().map(|d| d.join(&benchmark_name));
    let debug_mode = opt.debug;
    let env_str = envs
        .iter()
        .map(|(name, val)| format!("{name}={val}"))
        .collect::<Vec<String>>()
        .join(" ");
    let timeout = if let Some(case_timeout) = benchmark.timeout_secs {
        Duration::from_secs(case_timeout.min(opt.global_timeout_secs))
    } else {
        Duration::from_secs(opt.global_timeout_secs)
    };
    let cargo_dir = config.workdir.clone();
    let dry_run = opt.dry_run;
    let silent = opt.silent;
    let logical_and = opt.logical_and;

    move || {
        // using stupid timers to enforce launch order.
        thread::sleep(delay);

        let (ip, port) = (&host, "22");

        let mut cmd = Command::new("ssh");

        let mut stdout_writer = None;
        let mut stderr_writer = None;
        if let Some(output_dir) = output_dir {
            let stdout_file = output_dir
                .join(format!("{}_{}.log", worker.bin, ip))
                .with_extension("stdout");
            let stderr_file = output_dir
                .join(format!("{}_{}.log", worker.bin, ip))
                .with_extension("stderr");

            let stdout = open_with_create_append(stdout_file);
            let stderr = open_with_create_append(stderr_file);
            if silent {
                // This has less indirection and will be more efficient.
                cmd.stdout(stdout).stderr(stderr);
            } else {
                stdout_writer = Some(stdout);
                stderr_writer = Some(stderr);
                cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
            }
        } else {
            // collect the result to local stdout, similar to mpirun
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
            if silent {
                log::warn!(
                    "You may want to specify `--output=<output>` to capture the running log"
                );
            }
        }

        cmd.arg("-oStrictHostKeyChecking=no")
            // .arg("-tt") // force allocating a tty
            .arg("-oConnectTimeout=2")
            .arg("-oConnectionAttempts=3")
            .arg("-p")
            .arg(port)
            .arg(ip);

        // TODO(cjr): also to distribute binary program to workers
        let env_path = env::var("PATH").expect("failed to get PATH");
        if !debug_mode {
            cmd.arg(format!(
                // "export PATH={} && cd {} && {} numactl -N 0 -m 0 cargo run --release --bin {} -- {}",
                "export PATH={} && cd {} && {} target/release/{} {}",
                env_path,
                cargo_dir.display(),
                env_str,
                worker.bin,
                worker.args
            ));
        } else {
            cmd.arg(format!(
                "export PATH={} && cd {} && {} numactl -N 0 -m 0 target/debug/{} {}",
                env_path,
                cargo_dir.display(),
                env_str,
                worker.bin,
                worker.args
            ));
        }

        let cmd_str = get_command_str(&cmd);
        log::debug!("command: {}", cmd_str);

        let mut kill_cmd = Command::new("ssh");
        kill_cmd
            .arg("-oStrictHostKeyChecking=no")
            .arg("-oConnectTimeout=2")
            .arg("-oConnectionAttempts=3")
            .arg("-p")
            .arg(port)
            .arg(ip);
        kill_cmd.arg(format!("pkill -{} -f {}", worker.term_signal, worker.bin));

        if !dry_run {
            // poll command status until timeout or user Ctrl-C
            wait_command(
                cmd,
                Some(kill_cmd),
                timeout,
                &host,
                stdout_writer,
                stderr_writer,
                logical_and,
            )
            .unwrap();
        }
    }
}

// We assume a NFS setup, so we just build once and the products will be available to other
// machines.
fn build_all<A: AsRef<str>, P: AsRef<path::Path>>(
    binaries: impl IntoIterator<Item = A>,
    cargo_dir: P,
) -> anyhow::Result<()> {
    let manifest_path =
        path::Path::new(shellexpand::tilde(&cargo_dir.as_ref().to_string_lossy()).as_ref())
            .join("Cargo.toml");
    let args_bins: Vec<_> = binaries
        .into_iter()
        .flat_map(|b| vec!["--bin".to_owned(), b.as_ref().to_owned()])
        .collect();
    // format!("cd {cargo_dir}; cargo build --release {args_bins}");
    let mut cargo_build_cmd = Command::new("cargo");
    cargo_build_cmd
        .args([
            "build",
            "--release",
            "--manifest-path",
            manifest_path.to_string_lossy().as_ref(),
            "--workspace",
        ])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .args(&args_bins);

    let cmd_str = get_command_str(&cargo_build_cmd);
    log::debug!("building command: {}", cmd_str);

    let timeout_60s = Duration::from_secs(60);
    wait_command(cargo_build_cmd, None, timeout_60s, "", None, None, false)?;
    Ok(())
}

fn run_benchmark_group(opt: &Opt, group: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    use walkdir::WalkDir;

    let dir = opt.benchmark.clone();
    if !dir.exists() || !dir.is_dir() {
        return Err(anyhow::anyhow!("{:?} must be a directory", dir));
    }

    // find the matching benchmark cases
    let mut suites = Vec::new();
    for entry in WalkDir::new(dir).follow_links(true) {
        let entry = entry.unwrap();
        log::info!("traversing {}", entry.path().display());
        let path = path::PathBuf::from(entry.path());
        if path.is_dir() {
            continue;
        }

        let content = fs::read_to_string(&path).with_context(|| "read_to_string")?;
        match toml::from_str::<'_, Benchmark>(&content) {
            Ok(spec) => {
                if spec.group != group {
                    continue;
                }
            }
            Err(e) => log::warn!(
                "{} is not an valid benchmark case: {}, skipping...",
                path.display(),
                e
            ),
        }

        suites.push(path);
    }

    // run all matched benchmark cases
    let start = Instant::now();
    for p in &suites {
        match run_benchmark(opt, p.clone()) {
            Ok(()) => {}
            Err(e) => {
                log::error!("Failed to run benchmark {} because {}", p.display(), e);
            }
        }

        thread::sleep(Duration::from_millis(1000));

        let terminate_ts = (*TERMINATE.lock().unwrap()).unwrap_or(start);
        if terminate_ts > start {
            break;
        }
    }

    Ok(())
}

fn run_benchmark(opt: &Opt, path: path::PathBuf) -> anyhow::Result<()> {
    // read global config
    let config = Config::from_path(&opt.configfile)?;
    // read a single benchmark spec
    let content = fs::read_to_string(path)?;
    let spec: Benchmark = toml::from_str(&content)?;

    // create or clean output directory
    if let Some(d) = opt.output_dir.as_ref() {
        let output_dir = &d.join(&spec.name);
        if output_dir.exists() {
            // rm -r output_dir
            fs::remove_dir_all(output_dir)?;
        }
        fs::create_dir_all(output_dir)?;
    }

    // process envs
    let envs: Vec<(String, String)> = if let Some(env_table) = config.env.as_table() {
        env_table
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
            .collect()
    } else {
        panic!("unexpected config envs: {:?}", config.env);
    };

    // bulid benchmark cases
    let mut binaries: Vec<String> = spec.worker.iter().map(|s| s.bin.clone()).collect();
    binaries.dedup();
    let cargo_dir = &config.workdir;
    build_all(&binaries, cargo_dir)?;

    // print info
    log::info!(
        "{}: {}, description: {}",
        Green.bold().paint("Running benchmark"),
        spec.name,
        spec.description
    );

    // calculate a start time for each worker according to their topological order
    use std::collections::VecDeque;
    let n = spec.worker.len();
    let mut start_ts = vec![0; n];
    let mut g = vec![vec![]; n];
    let mut indegree = vec![0; n];
    let mut queue = VecDeque::new();
    for (i, w) in spec.worker.iter().enumerate() {
        indegree[i] = w.dependencies.len();
        if w.dependencies.is_empty() {
            queue.push_back(i);
        }
        for &j in &w.dependencies {
            g[j].push(i);
        }
    }
    while let Some(x) = queue.pop_front() {
        log::trace!("x: {}, start_ts[x]: {}", x, start_ts[x]);
        for &y in &g[x] {
            log::trace!("(x -> y): ({} -> {}), start_ts[y]: {}", x, y, start_ts[y]);
            start_ts[y] = start_ts[y].max(start_ts[x] + 1);
            indegree[y] -= 1;
            if indegree[y] == 0 {
                queue.push_back(y);
            }
        }
    }

    log::debug!("start_ts: {:?}", start_ts);

    // start workers based on their start_ts
    let mut handles = vec![];
    for (i, w) in spec.worker.iter().enumerate() {
        let delay = Duration::from_millis(start_ts[i] * spec.start_delay * 1000);
        let h = thread::spawn(start_ssh(opt, &spec, w.clone(), &config, &envs, delay));
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
use std::sync::Mutex;

lazy_static::lazy_static! {
    static ref TERMINATE: Mutex<Option<Instant>> = Mutex::new(None);
}

extern "C" fn handle_sigint(sig: i32) {
    log::warn!("sigint catched");
    assert_eq!(sig, signal::SIGINT as i32);
    *TERMINATE.lock().unwrap() = Some(Instant::now());
}

mod cleanup {
    use super::*;

    pub(crate) struct SttySane;

    impl SttySane {
        pub(crate) fn new() -> Self {
            Self::stty_sane();
            SttySane
        }

        fn stty_sane() {
            Command::new("stty")
                .arg("sane")
                .spawn()
                .expect("stty sane failed to start")
                .wait()
                .expect("stty sane failed to execute");
        }
    }

    impl Drop for SttySane {
        fn drop(&mut self) {
            Self::stty_sane();
        }
    }
}

fn main() {
    let _guard = cleanup::SttySane::new();
    init_env_log("RUST_LOG", "info");

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

    if let Some(g) = opt.group.clone() {
        // opt.benchmark should points to a directory
        run_benchmark_group(&opt, &g).unwrap_or_else(|e| panic!("run_benchmark_group failed: {e}"));
    } else {
        // opt.benchmark should points to a file
        let path = opt.benchmark.clone();
        run_benchmark(&opt, path).unwrap_or_else(|e| panic!("run_benchmark failed: {e}"));
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
