//! Inspired by Theseus cargo.
//! https://github.com/theseus-os/Theseus/blob/89489db4a11f2b0ea398d72740a0258111390f5f/tools/theseus_cargo/src/main.rs
//!
//! Different than theseus_cargo, phoenix_cargo
//! + supports dylib and proc_macro
//! + supports choosing a crate among multiple builds (with different versions or features)
//! of the same dependency crate.
//! - it does not currenlty handle cross-compiling
use std::collections::{BTreeSet, HashMap};
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use anyhow::{bail, Context};
use clap::Parser;
use semver::{Version, VersionReq};

#[derive(Clone)]
struct Available {
    inner: Arc<AvailableInner>,
}

impl fmt::Debug for Available {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Available")
            .field("data", &self.inner.lock)
            .finish()
    }
}

impl Default for Available {
    fn default() -> Self {
        Self::new()
    }
}

impl Available {
    fn new() -> Self {
        Available {
            inner: Arc::new(AvailableInner {
                lock: Mutex::new(false),
                cvar: Condvar::new(),
            }),
        }
    }

    fn wait(&self) {
        let AvailableInner { lock, cvar } = &*self.inner;
        let mut available = lock.lock().unwrap();
        while !*available {
            available = cvar.wait(available).unwrap();
        }
    }

    fn make_available(&self) {
        let AvailableInner { lock, cvar } = &*self.inner;
        let mut available = lock.lock().unwrap();
        *available = true;
        cvar.notify_all();
    }
}

#[derive(Debug)]
struct AvailableInner {
    lock: Mutex<bool>,
    cvar: Condvar,
}

#[derive(Debug, Clone)]
struct Crate {
    name: String,
    metadata: String,
    pkg_version: Version,
    features: Vec<String>,
    path: PathBuf,
    // direct dependencies, each is a name with hash suffix/metadata
    dependencies: Vec<String>,
    is_primary: bool,
    // initialize it later
    is_recreated: Option<bool>,
    // whether the crate has been available
    available: Available,
}

impl Crate {
    fn crate_name_with_hash(&self) -> String {
        format!("{}-{}", self.name, self.metadata)
    }
}

/// It maps a crate-name to a list of candidate crates.
#[derive(Debug, Clone)]
struct PrebuiltCrates(HashMap<String, Vec<Crate>>);

impl PrebuiltCrates {
    fn new(rustc_commands: &[String], host_dep: &Path) -> anyhow::Result<Self> {
        let mut crates = HashMap::default();
        for original_cmd in rustc_commands {
            let Some(mut c) = get_crate_from_rustc_command(original_cmd)? else {
                continue;
            };
            c.is_recreated = Some(true);
            c.path = host_dep.join(c.path.file_name().context("Could not get file_name")?);
            c.available.make_available();
            crates
                .entry(c.name.to_owned())
                .or_insert_with(Vec::new)
                .push(c);
        }
        Ok(Self(crates))
    }
}

#[derive(Debug)]
struct CompileDb {
    /// The directory that contains the prebuilt crates for phoenix_common.
    prebuilt_dir: PathBuf,

    /// The dependency closure for phoenix_common
    prebuilt_crate_sets: PrebuiltCrates,

    /// Lookup table that returns the crate information for a given cratename-metadata
    crate_info: HashMap<String, Crate>,
}

impl CompileDb {
    fn from_file(opts: &Opts) -> anyhow::Result<Self> {
        let compile_log_path = opts.compile_log.as_ref().unwrap().as_path();

        let host_dep = fs::canonicalize(&opts.host_dep.as_ref().unwrap()).with_context(|| {
            format!(
                "--host-dep arg '{}' was invalid path.",
                opts.host_dep.as_ref().unwrap().display()
            )
        })?;

        let copy_to = host_dep.as_ref();

        // Parse rustc commands
        let compile_log_file = fs::File::open(compile_log_path)?;
        let mut reader = BufReader::new(compile_log_file);
        let rustc_commands = capture_rustc_commands(&mut reader, 0);

        // Extract the prebuilt_dir from the last command
        // let last_cmd = rustc_commands
        //     .last()
        //     .context("No commands captured from stderr during the initial cargo command")?;
        // let prebuilt_dir = PathBuf::from(get_out_dir_arg(last_cmd)?);

        // let prebuilt_dir = fs::canonicalize(&prebuilt_dir).with_context(|| {
        //     format!("--input arg '{}' was invalid path.", prebuilt_dir.display())
        // })?;

        // Scan all the rustc commands and build the index to the crates
        let prebuilt_crate_sets = PrebuiltCrates::new(&rustc_commands, copy_to)?;

        // Organize the crates in prebuilt set for lookup
        let crate_info = prebuilt_crate_sets
            .0
            .values()
            .flat_map(|crates| {
                crates
                    .iter()
                    .map(|c| (format!("{}-{}", c.name, c.metadata), c.clone()))
            })
            .collect();
        Ok(Self {
            prebuilt_dir: copy_to.to_path_buf(),
            prebuilt_crate_sets,
            crate_info,
        })
    }

    fn mark_recreated(&mut self, c: &Crate, is_recreated: bool) {
        self.crate_info
            .get_mut(&c.crate_name_with_hash())
            .unwrap_or_else(|| panic!("Not found info for crate: {:?}", c))
            .is_recreated = Some(is_recreated);
    }

    fn insert_crate(&mut self, c: &Crate) {
        self.crate_info.insert(c.crate_name_with_hash(), c.clone());
        // .ok_or(())
        // .unwrap_err();
    }

    fn get_crate(&self, crate_name_with_hash: &str) -> Option<Crate> {
        self.crate_info.get(crate_name_with_hash).cloned()
    }

    // Two crates are _compatible_ if they meets the following conditions:
    // 1. they have the exact same crate name
    // 2. their semantic versions are compatible (check more out on semantic version)
    // 3. the feature set of `desired` is a subset of `provided`
    // 4. their direct dependencies are also _compatible_.
    fn is_compatible(&self, desired: &Crate, provided: &Crate, recurse_level: usize) -> bool {
        let req = VersionReq::parse(&desired.pkg_version.to_string()).unwrap();
        if !req.matches(&provided.pkg_version) {
            println!(
                "version not compatible: desired {:?}, provided {:?}",
                desired, provided
            );
            return false;
        }
        let desired_features: BTreeSet<_> = desired.features.iter().cloned().collect();
        let provided_features: BTreeSet<_> = provided.features.iter().cloned().collect();
        if !provided_features.is_superset(&desired_features) {
            println!(
                "features not compatible: desired {:?}, provided {:?}",
                desired, provided
            );
            return false;
        }
        if recurse_level == 0 {
            // The implementation here does not check the compatibability of each crate exactly.
            // Instead, it just checks whether a compatible one can be found in the prebuilt_set
            // for all direct dependencies.
            desired.dependencies.iter().all(|dep| {
                self.get_crate(&dep)
                    .map(|dep_crate| {
                        self.contains_compatible_crates_in_prebuilt(&dep_crate, recurse_level + 1)
                    })
                    .unwrap_or(false)
            })
        } else {
            true
        }
    }

    fn contains_compatible_crates_in_prebuilt(&self, c: &Crate, recurse_level: usize) -> bool {
        // TODO(cjr): Accelerate this function using a query cache.
        self.prebuilt_crate_sets
            .0
            .get(&c.name)
            .map(|candidate_set| {
                candidate_set
                    .iter()
                    .any(|cand| self.is_compatible(c, cand, recurse_level))
            })
            .unwrap_or(false)
    }

    fn find_compatible_crates_in_prebuilt(&self, c: &Crate) -> Vec<Crate> {
        self.prebuilt_crate_sets
            .0
            .get(&c.name)
            .map(|candidate_set| {
                let mut cands = Vec::new();
                for cand in candidate_set {
                    println!("cand: {:?}", cand);
                    if self.is_compatible(c, cand, 0) {
                        cands.push(cand.clone());
                    }
                }
                cands
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Parser)]
#[command(
    about = "A wrapper around cargo to support out-of-tree build of phoenix plugins \
    based on a previous build of phoenix."
)]
struct Opts {
    /// The dep file that specifies the dependencies of a latest build of phoenix_common.
    ///
    /// If not specified, it will be the phoenix/phoneix_compile_log.txt under the target_dir
    /// for this build.
    #[arg(long)]
    compile_log: Option<PathBuf>,

    /// The path to the phoenix_common dependencies we will copy to.
    ///
    /// If not specified, it will be the phoenix/host_dep under the target_dir for this build.
    #[arg(long)]
    host_dep: Option<PathBuf>,

    /// Cargo subcommand
    #[arg(raw = true, allow_hyphen_values = true)]
    cargo_subcommand: Vec<String>,
}

fn is_build_command(cargo_subcommand: &[String]) -> bool {
    match cargo_subcommand.first().map(|x| x.as_str()) {
        Some("build") | Some("b") => true,
        _ => false,
    }
}

/// Returns the project's workspace root.
///
/// It is equivalent to running cargo locate-project --workspace [--manifest-path some_path].
fn locate_project_root(cargo_subcommand: &[String]) -> anyhow::Result<PathBuf> {
    let mut cmd = Command::new("cargo");
    cmd.arg("locate-project").arg("--workspace");

    // Forward the --manifest-path option if present
    if let Some(manifest_path) = cargo_subcommand
        .iter()
        .position(|arg| arg == "--manifest-path")
        .map(|pos| cargo_subcommand[pos + 1].clone())
    {
        cmd.arg("--manifest-path").arg(manifest_path);
    }

    let output = cmd.output().with_context(|| {
        format!(
            "Failed to run cargo command: {:?} {:?}",
            cmd.get_program(),
            cmd.get_args()
        )
    })?;

    let output_json = String::from_utf8(output.stdout)?;

    // Extract the path inside the json
    let manifest_path = output_json
        .trim()
        .strip_prefix(r#"{"root":""#)
        .and_then(|s| s.strip_suffix(r#""}"#))
        .with_context(|| format!("Unexpected output: {}", output_json))?;

    let manifest_path = PathBuf::from(manifest_path);
    let bcx_root = manifest_path.parent().with_context(|| {
        format!(
            "Unable to get parent directory for manifest: {}",
            manifest_path.display(),
        )
    })?;

    Ok(bcx_root.to_path_buf())
}

/// Returns the target-dir for this build.
///
/// It returns the value of `--target-dir` is it is present. Otherwise, it returns bcx_root/target.
fn locate_target_dir(cargo_subcommand: &[String], bcx_root: &Path) -> PathBuf {
    // determine the target-dir in the following order
    // 1. --target-dir command-line flag
    // 2. build.target-dir config value
    // 3. env CARGO_TARGET_DIR/CARGO_BUILD_TARGET_DIR
    // 4. default: bcx_root/target
    cargo_subcommand
        .iter()
        .position(|arg| arg == "--target-dir")
        .map(|pos| PathBuf::from(&cargo_subcommand[pos + 1]))
        .unwrap_or_else(|| bcx_root.join("target").join("phoenix"))

    // let cargo_config = cargo::util::config::Config::default()?;
    // cargo_config.configure(
    //     0,
    //     false,
    //     None,
    //     false,
    //     false,
    //     true,
    //     &args_target_dir,
    //     &[],
    //     &[],
    // )?;

    // let ws = cargo::core::Workspace::new(&bcx_root.join("Cargo.toml"), &cargo_config)?;
    // Ok(PathBuf::new())
}

fn determine_profile_dir(cargo_subcommand: &[String]) -> String {
    // I couldn't find a more reliable yet simple way to do this unless follow the cargo's source
    // code to parse, initialize workspace, and expand the command alias
    if cargo_subcommand
        .iter()
        .find(|s| s.as_str() == "--release")
        .is_some()
        || cargo_subcommand
            .iter()
            .find(|s| s.as_str() == "-r")
            .is_some()
    {
        "release".to_owned()
    } else {
        "debug".to_owned()
    }
}

fn main() -> anyhow::Result<()> {
    let mut opts = Opts::parse();

    // Determine the cargo root workspace directory
    // dbg!(&opts.cargo_subcommand);
    let cargo_bcx_root = locate_project_root(&opts.cargo_subcommand)?;

    // First pass, do not touch .fingerprint, set RUSTC_WRAPPER=echo, and capture the stderr
    // Since we could run from a last failed or interrupted build, we need to recover the old
    // fingerprint directory.
    let target_dir = locate_target_dir(&opts.cargo_subcommand, &cargo_bcx_root);
    let profile_dir = determine_profile_dir(&opts.cargo_subcommand);

    // Adjust `compile_log` and `host_dep` according to cargo_subcommand if not set by the user
    if opts.compile_log.is_none() {
        opts.compile_log = Some(target_dir.join("phoenix_compile_log.txt"));
    }
    if opts.host_dep.is_none() {
        opts.host_dep = Some(target_dir.join("host_dep"));
    }

    // Initialize the compile database from the cargo's log file.
    // This log file should be the file generated during building package phoenix_common.
    let mut compile_db = CompileDb::from_file(&opts)?;

    dbg!(&compile_db);

    let verbose_count = count_verbose_arg(&opts.cargo_subcommand);

    // phoenix_cargo builds in three passes.
    recover_fingerprint_directory(&target_dir, &profile_dir)?;
    // let first_pass_stderr_captured = run_initial_cargo(&opts.cargo_subcommand, verbose_count)?;

    // if !is_build_command(&opts.cargo_subcommand) {
    //     println!("Exiting after completing non-'build' cargo command.");
    //     return Ok(());
    // }

    // Second pass, rename .fingerprint and do a fresh build, capture the full compilation
    // information and rename .fingerprint back
    // Determine the target dir for this build
    let second_pass_stderr_captured = {
        // Here, temporarily remove the ".fingerprint/` directory, in order to force rustc
        // to rebuild all artifacts for all of the modified rustc commands to capture the
        // informatino to build compile db.
        let _guard = FingerprintDirGuard::new(&target_dir, &profile_dir)?;

        run_initial_cargo(&opts.cargo_subcommand, verbose_count, &target_dir)?
    };

    if !is_build_command(&opts.cargo_subcommand) {
        println!("Exiting after completing non-'build' cargo command.");
        return Ok(());
    }

    {
        // Final pass, re-run the commands captured in the first stage, modify the extern arguments
        // based on the matched crates in the compile database.

        // Change working directory
        if let Some(manifest_path) = opts
            .cargo_subcommand
            .iter()
            .position(|arg| arg == "--manifest-path")
            .map(|pos| opts.cargo_subcommand[pos + 1].clone())
        {
            let manifest_path = fs::canonicalize(&manifest_path)?;
            let manifest_dir = manifest_path.parent().with_context(|| {
                format!(
                    "manifest_path has no parent directory: {}",
                    manifest_path.display()
                )
            })?;
            std::env::set_current_dir(manifest_dir)?;
        }

        // Now that we have run the initial cargo build, it has created many redundant dependency artifacts
        // in the local crate's target/ directory, namely the locally re-built versions of phoenix crates,
        // specifically all the crates that are in the set of prebuilt crates.
        // We need to remove those redundant files from the local target/ directory (the "out-dir")
        // such that when we re-issue the rustc commands below, it won't fail with an error about
        // multiple "potentially newer" versions of a given crate dependency.
        let last_cmd = second_pass_stderr_captured
            .last()
            .context("No commands captured from stderr during the initial cargo command")?;
        let out_dir = PathBuf::from(get_out_dir_arg(last_cmd)?);

        remove_redundant_artifacts(&compile_db, out_dir)?;

        // Re-execute the rustc commands that we captured from the original cargo verbose output.
        rayon::scope(|s| {
            for original_cmd in &second_pass_stderr_captured {
                // This function will only re-run rustc for crates that don't already exist in the set of prebuilt crates.
                if let Some(mut task) = run_rustc_command(original_cmd, &mut compile_db).unwrap() {
                    s.spawn(move |_s| {
                        for dep in task.dependencies {
                            dep.1.wait();
                        }

                        // Finally, we run the recreated rustc command.
                        let mut rustc_process = task
                            .recreated_cmd
                            .spawn()
                            .expect("Failed to run cargo command");
                        let exit_status = rustc_process.wait().expect("Error running rustc");

                        match exit_status.code() {
                            Some(0) => {
                                println!(
                                    "{} {}: Ran rustc command (modified for Phoenix) successfully.",
                                    task.c.name, task.c.pkg_version
                                );

                                // Copy the compilation result to the parent directory of deps, just like what cargo
                                // would do.
                                if task.c.is_primary {
                                    copy_result(&task.c).unwrap();
                                }

                                task.c.available.make_available();
                            }
                            Some(code) => panic!("rustc command exited with failure code {}", code),
                            _ => panic!("rustc command failed and was killed."),
                        }
                    });
                }
            }
        });
    }

    Ok(())
}

// The commands we care about capturing starting with "Running `" and end with "`".
const COMMAND_COMPILING: &str = "Compiling ";
const COMMAND_START: &str = "Running `";
const COMMAND_END: &str = "`";
const RUSTC_CMD_START: &str = "rustc --crate-name";
const BUILD_SCRIPT_CRATE_NAME: &str = "build_script_build";

const CARGO_PKG_VERSION: &str = "CARGO_PKG_VERSION";
const CARGO_PRIMARY_PKG: &str = "CARGO_PRIMARY_PACKAGE=1";

// The format of rmeta/rlib file names.
const RMETA_RLIB_FILE_PREFIX: &str = "lib";
const RMETA_FILE_EXTENSION: &str = "rmeta";
const RLIB_FILE_EXTENSION: &str = "rlib";
const DYLIB_FILE_EXTENSION: &str = "so";
const PREFIX_END: usize = RMETA_RLIB_FILE_PREFIX.len();

// Captures the `Running` rustc commands printed by cargo. Prints only the user desired output.
fn capture_rustc_commands<R: io::Read>(
    reader: &mut BufReader<R>,
    verbose_level: usize,
) -> Vec<String> {
    let mut captured_commands = Vec::new();

    // Use regex to strip out the ANSI color codes emitted by the cargo command
    let ansi_escape_regex = regex::Regex::new(r"[\x1B\x9B]\[[^m]+m").unwrap();

    let mut pending_multiline_cmd = false;
    let mut original_multiline = String::new();
    let mut is_primary_pkg = false;

    // Capture every line that cargo writes to stderr.
    // We only re-echo the lines that should be outputted by the verbose level specified.
    // The complexity below is due to the fact that a verbose command printed by cargo
    // may span multiple lines, so we need to detect the beginning and end of a multi-line command
    // and merge it into a single line in our captured output.
    reader
        .lines()
        .filter_map(|line| line.ok())
        .for_each(|original_line| {
            let replaced = ansi_escape_regex.replace_all(&original_line, "");
            let line_stripped = replaced.trim_start();

            let is_final_line = (line_stripped.contains("--crate-name")
                && line_stripped.contains("--crate-type"))
                || line_stripped.ends_with("build-script-build`");

            if line_stripped.starts_with(COMMAND_START) {
                // Here, we've reached the beginning of a rustc command,
                // which we actually do care about.
                is_primary_pkg = false;
                is_primary_pkg |= line_stripped.contains(CARGO_PRIMARY_PKG);
                captured_commands.push(line_stripped.to_string());
                pending_multiline_cmd = !is_final_line;
                original_multiline = String::from(&original_line);
                if !is_final_line {
                    return; // continue to the next line
                }
            } else {
                // Here, we've reached another line, which *may* be the continuation of
                // a previous rustc command, or it may just be a completely irrelevant
                // line of output.
                is_primary_pkg |= line_stripped.contains(CARGO_PRIMARY_PKG);
                if pending_multiline_cmd {
                    // append to the latest line of output instead of adding a new line
                    let last = captured_commands
                        .last_mut()
                        .expect("BUG: captured_commands had no last element");
                    last.push(' ');
                    last.push_str(line_stripped);
                    original_multiline.push('\n');
                    original_multiline.push_str(&original_line);
                    pending_multiline_cmd = !is_final_line;
                    if !is_final_line {
                        return; // continue to the next line
                    }
                } else {
                    // Here: this is an unrelated line of output that isn't a command we want
                    // to capture.
                    original_multiline.clear(); // = String::from(&original_line);
                }
            }

            // In the above cargo command, we added a verbose argument to capture the commands
            // issued from cargo to rustc.
            // But if the user didn't ask for that, then we shouldn't print that verbose output here.
            // Verbose output lines start with "Running `", "+ ", or "[".
            let should_print = |stripped_line: &str, is_primary: bool| {
                // println!(
                //     "debuggin: verbose_level: {}, stripped_line: {}",
                //     verbose_level,
                //     &stripped_line[0..30.min(stripped_line.len())]
                // );
                // TODO(cjr): cargo displays warnings for local package, but I did not find a
                // convenient way to determine if a package is local, so here we just show
                // warnings for primary packages we are building.
                let show_warnings = is_primary
                    && !stripped_line.starts_with("+ ")
                    && !stripped_line.starts_with("[")
                    && !stripped_line.starts_with(COMMAND_START);
                match verbose_level {
                    0 => stripped_line.starts_with(COMMAND_COMPILING) || show_warnings,
                    1 => {
                        // print only "Compiling" and warning/error lines if not verbose
                        stripped_line.starts_with(COMMAND_COMPILING)
                            || stripped_line.starts_with(COMMAND_START)
                            || show_warnings
                    }
                    2.. => true, // print everything if verbose
                    _ => panic!("negative verbose_level: {}", verbose_level),
                }
            };

            if !original_multiline.is_empty() && is_final_line {
                let original_multiline_replaced =
                    ansi_escape_regex.replace_all(&original_multiline, "");
                let original_multiline_stripped = original_multiline_replaced.trim_start();
                if should_print(original_multiline_stripped, is_primary_pkg) {
                    eprintln!("{}", original_multiline)
                }
            } else if should_print(line_stripped, is_primary_pkg) {
                eprintln!("{}", original_line);
            }
        });

    captured_commands
}

/// Runs the actual cargo build command.
///
/// Returns the captured content of content written to `stderr` by the cargo command, as a list of lines.
fn run_initial_cargo<P: AsRef<Path>>(
    full_args: &[String],
    verbose_level: usize,
    target_dir: P,
) -> anyhow::Result<Vec<String>> {
    let subcommand = full_args
        .first()
        .context("Missing subcommand argument to `phoenix_cargo` (e.g., `build`)")?;

    if !is_build_command(full_args) {
        bail!(
            "cargo commands other than `build` are not supported. \
            You tried to run subcommand {:?}.",
            subcommand
        );
    }

    let mut cmd = Command::new("cargo");
    cmd.arg(subcommand)
        .stderr(Stdio::piped())
        .stdout(Stdio::piped());

    for arg in &full_args[1..] {
        cmd.arg(arg);
    }

    // TODO: Ensure that we use only the arguments specified by the phoenix build config
    // cmd.args(shlex::split(build_config.cargoflags).unwrap())
    //     .arg("--target").arg(&build_config.target);

    // Ensure that we run the cargo command with the maximum verbosity level, which is -vv.
    cmd.arg("-vv");

    // Use full color output to get a regular terminal-esque display from cargo
    cmd.arg("--color=always");

    // RUSTC_WRAPPER=echo
    // cmd.env("RUSTC_WRAPPER", "echo");

    cmd.env("CARGO_TARGET_DIR", target_dir.as_ref());

    // TODO: Add the requisite environment variables to configure cargo such that rustc builds with the
    // proper config.
    // cmd.env("RUST_TARGET_PATH");

    // Cargo will directly use the rustflags read from .cargo/config.toml if RUSTFLAGS not set.
    // Add the sysroot argument to our rustflags so cargo will use our pre-built phoenix dependencies.
    // let mut rustflags = format!("--sysroot {}", sysroot_dir_path.display());
    // let mut rustflags = String::new();

    // -Zbinary-dep-depinfo allows us to track dependencies of each rlib
    // rustflags.push_str(" -Zunstable-options -Zbinary-dep-depinfo");
    // cmd.env("RUSTFLAGS", rustflags);

    println!("\nRunning initial cargo command:\n{:?}", cmd);
    cmd.get_envs()
        .for_each(|(k, v)| println!("\t### env {:?} = {:?}", k, v));

    // Run the actual cargo command.
    let mut child_process = cmd.spawn().context("Failed to run cargo command.")?;

    // We read the stderr output in this thread and create a new thread to thread the stdout
    // output.
    let stdout = child_process
        .stdout
        .take()
        .context("Could not capture stdout")?;
    let t = thread::spawn(move || {
        let stdout_reader = BufReader::new(stdout);
        let mut stdout_logs = Vec::new();
        stdout_reader
            .lines()
            .filter_map(|line| line.ok())
            .for_each(|line| {
                // Cargo only prints to stdout for build script output only if very verbose.
                if verbose_level >= 2 {
                    println!("{}", line);
                }
                stdout_logs.push(line);
            });
        stdout_logs
    });

    let stderr = child_process
        .stderr
        .take()
        .context("Could not capture stderr.")?;
    let mut stderr_reader = BufReader::new(stderr);
    let stderr_logs = capture_rustc_commands(&mut stderr_reader, verbose_level);

    let _stdout_logs = t.join().unwrap();
    let exit_status = child_process
        .wait()
        .context("Failed to wait for cargo process to finish")?;
    match exit_status.code() {
        Some(0) => {}
        Some(code) => bail!("cargo command completed with failed exit code {}", code),
        _ => bail!("cargo command was killed"),
    }

    Ok(stderr_logs)
}

/// Returns true if the given `arg` should be ignored in our rustc invocation.
fn ignore_arg(arg: &str) -> bool {
    arg == "--error-format" || arg == "--json"
}

fn parse_rustc_command(
    original_cmd: &str,
) -> anyhow::Result<Option<(&str, &str, clap::ArgMatches)>> {
    let command = if original_cmd.starts_with(COMMAND_START) && original_cmd.ends_with(COMMAND_END)
    {
        let end_index = original_cmd.len() - COMMAND_END.len();
        &original_cmd[COMMAND_START.len()..end_index]
    } else {
        bail!(
            "Unexpected formatting in capture command (must start with {:?} and end with {:?}. \
            Command: {:?}",
            original_cmd,
            COMMAND_START,
            COMMAND_END,
        );
    };

    // Skip invocations of build scripts, as I don't think we need to re-run those.
    // If this turns out to be wrong and we do need to run them, we need to change this logic to simply re-run it
    // and skip pretty much the rest of this entire function.
    if command.ends_with("build-script-build") {
        return Ok(None);
    }

    let start_of_rustc_cmd = command.find(RUSTC_CMD_START).with_context(|| {
        format!(
            "Couldn't find {:?} in command:\n{:?}",
            RUSTC_CMD_START, command
        )
    })?;
    let rustc_env_vars = &command[..start_of_rustc_cmd];
    let command_without_env = &command[start_of_rustc_cmd..];

    // The arguments in the command that we care about are:
    //  *  "-L dependency=<dir>"
    //  *  "--extern <crate_name>=<crate_file>.rmeta"
    //
    // Below, we use `clap` to find those argumnets and replace them.
    //
    // First, we parse the following part:
    // "rustc --crate-name <crate_name> <crate_source_file> <all_other_args>"
    let top_level_matches = rustc_clap_options("rustc")
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .allow_external_subcommands(true)
        .color(clap::ColorChoice::Never)
        .try_get_matches_from(shlex::split(command_without_env).unwrap());

    let top_level_matches = top_level_matches
        .context("Missing support for argument found in captured rustc command")?;

    Ok(Some((
        rustc_env_vars,
        command_without_env,
        top_level_matches,
    )))
}

fn get_crate_from_rustc_command(original_cmd: &str) -> anyhow::Result<Option<Crate>> {
    let Some((rustc_env_vars, command_without_env, top_level_matches)) =
                parse_rustc_command(original_cmd)? else {
                // skip invocations of build scripts
                return Ok(None);
            };

    // crate-name
    // Clap will parse the args as such:
    // * the --crate-name will be the first argument
    // * the path to the crate's main file will be the first subcommand
    // * that subcommand's arguments will include ALL OTHER arguments that we care about, specified below.
    let crate_name = top_level_matches
        .get_one::<String>("--crate-name")
        .expect("rustc command did not have required --crate-name argument");

    // pkg_version
    let splitted = shlex::split(&rustc_env_vars);
    let (_key, pkg_version) = splitted
        .as_ref()
        .unwrap()
        .iter()
        .map(|env| env.split_once('=').unwrap())
        .find(|&(k, _v)| k == CARGO_PKG_VERSION)
        .with_context(|| {
            format!(
                "Could not find {CARGO_PKG_VERSION} in envs: {:?}",
                rustc_env_vars
            )
        })?;

    let is_primary = rustc_env_vars.contains(CARGO_PRIMARY_PKG);

    // metadata
    let (_crate_source_file, additional_args) = top_level_matches
        .subcommand()
        .context("Missing crate source files and addition args after rustc")?;
    let args_after_source_file = additional_args.get_many::<OsString>("").unwrap();

    let matches = rustc_clap_options("")
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .allow_external_subcommands(true)
        .color(clap::ColorChoice::Never)
        .try_get_matches_from(args_after_source_file);

    let matches =
        matches.context("Missing support for argument found in captured rustc command")?;

    let crate_type = matches
        .get_one::<String>("--crate-type")
        .expect("rustc command did not have required --crate-type argument");

    let codegen_opts = matches
        .get_many::<String>("-C")
        .expect("rustc command did not have required -C argument");
    let metadata = codegen_opts
        .into_iter()
        .find_map(|opt| opt.strip_prefix("metadata="))
        .context("rustc command did not have metadata specified")?;

    // features
    let mut features = Vec::new();
    if let Some(values) = matches.get_many::<String>("--cfg") {
        for value in values {
            dbg!(value);
            if let Some(feature) = value.strip_prefix("feature=") {
                features.push(feature.to_owned());
            }
        }
    }

    // crate path
    let out_dir = PathBuf::from(get_out_dir_arg(command_without_env)?);
    let path = match crate_type.as_str() {
        "bin" => out_dir.join(format!("{}-{}", crate_name, metadata)),
        "lib" | "rlib" => out_dir.join(format!(
            "{}{}-{}.rlib",
            RMETA_RLIB_FILE_PREFIX, crate_name, metadata
        )),
        "proc-macro" | "dylib" => out_dir.join(format!(
            "{}{}-{}.so",
            RMETA_RLIB_FILE_PREFIX, crate_name, metadata
        )),
        _ => {
            panic!("Todo: support this crate-type: {}", crate_type)
        }
    };

    // direct dependencies
    let mut dependencies = Vec::new();
    if let Some(values) = matches.get_many::<String>("--extern") {
        for value in values {
            if value == "proc_macro" {
                dependencies.push(value.clone());
            } else {
                if let Some((_crate_name, crate_path)) = value.split_once('=') {
                    let crate_path = Path::new(crate_path);
                    let crate_name_with_hash = get_crate_name_with_hash_from_path(crate_path)?
                        .strip_prefix(RMETA_RLIB_FILE_PREFIX)
                        .with_context(|| {
                            format!(
                                "Found .rlib or .rmeta file after '--extern' \
                                    that didn't start with 'lib' prefix: {}",
                                crate_path.display()
                            )
                        })?;
                    dependencies.push(crate_name_with_hash.to_owned());
                } else {
                    panic!("Found --extern '{}' that does not have a exact path", value);
                }
            }
        }
    }

    Ok(Some(Crate {
        name: crate_name.to_owned(),
        metadata: metadata.to_owned(),
        pkg_version: Version::parse(pkg_version)?,
        features,
        path,
        dependencies,
        is_primary,
        is_recreated: None,
        available: Available::new(),
    }))
}

struct RustcTask {
    recreated_cmd: Command,
    c: Crate,
    dependencies: Vec<(String, Available)>,
}

/// Takes the given `original_cmd` that was captured from the verbose output of cargo,
/// and parses/modifies it to link against (depend on) the corresponding crate of the same name
/// from the list of prebuilt crates.
///
/// The actual dependency files (.rmeta/.rlib) for the prebuilt crates should be located in the
/// `prebuilt_dir`.
/// The target specification JSON file should be found in the `target_dir_path`.
/// These two directories are usually the same directory.
///
/// # Return
/// * Returns `Ok(task)` that contains that rustc task about to execute.
/// * Returns `Ok(None)` if no action needs to be taken.
///   This occurs if `original_cmd` is for building a build script (currently ignored),
///   or if `original_cmd` is for building a crate that already exists in the set of `prebuilt_crates`.
/// * Returns an error if the command fails to parse.
fn run_rustc_command(
    original_cmd: &str,
    compile_db: &mut CompileDb,
) -> anyhow::Result<Option<RustcTask>> {
    let prebuilt_dir = compile_db.prebuilt_dir.clone();

    let Some(c) = get_crate_from_rustc_command(original_cmd)? else {
        // skip invocations of build scripts
        return Ok(None);
    };

    compile_db.insert_crate(&c);

    let crate_name_with_hash = c.crate_name_with_hash();

    // Skip build script invocations, as we may not need to re-run those.
    if c.name == BUILD_SCRIPT_CRATE_NAME {
        println!("\n### Skipping build script build");
        return Ok(None);
    }

    // Skip crates that have already been built. (Not sure if this is always 100% correct)
    let crate_to_build = compile_db
        .get_crate(&crate_name_with_hash)
        .unwrap_or_else(|| panic!("Found no crate named: {:?}", crate_name_with_hash));
    if !compile_db
        .find_compatible_crates_in_prebuilt(&crate_to_build)
        .is_empty()
    {
        println!(
            "\n### Skipping already-built crate {:?}",
            crate_name_with_hash
        );
        compile_db.mark_recreated(&c, false);
        return Ok(None);
    }

    println!("\n\nLooking at original command:\n{}", original_cmd);
    let Some((rustc_env_vars, _command_without_env, top_level_matches)) =
        parse_rustc_command(original_cmd)? else {
        // skip invocations of build scripts
        return Ok(None);
    };

    let (crate_source_file, additional_args) = top_level_matches
        .subcommand()
        .context("Missing crate source files and addition args after rustc")?;

    // Now, re-create the rustc command invocation with the proper arguments.
    // First, we handle the --crate-name and --edition arguments, which may come before the crate source file path.
    let mut recreated_cmd = Command::new("rustc");
    recreated_cmd.arg("--crate-name").arg(&c.name);
    if let Some(edition) = top_level_matches.get_one::<String>("--edition") {
        recreated_cmd.arg("--edition").arg(edition);
    }
    recreated_cmd.arg(crate_source_file);

    let args_after_source_file = additional_args.get_many::<OsString>("").unwrap();

    // Second, we parse all other args in the command that followed the crate source file.
    // Note that the arg name, the parameter in with_name(), in each arg below MUST BE exactly how it is invoked by cargo.
    let matches = rustc_clap_options("")
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .allow_external_subcommands(true)
        .color(clap::ColorChoice::Never)
        .try_get_matches_from(args_after_source_file);

    let matches =
        matches.context("Missing support for argument found in captured rustc command")?;

    let mut dependencies = Vec::new();
    let mut args_or_deps_changed = false;

    // After adding the initial stuff: rustc command, crate name, (optional --edition), and crate source file,
    // the other arguments are added in the loop below.
    for arg in matches.ids() {
        let values = matches
            .get_raw(arg.as_str())
            .unwrap()
            .map(|s| s.to_os_string())
            .collect::<Vec<_>>();
        println!("Arg {:?} has values:\n\t {:?}", arg, values);
        if ignore_arg(arg.as_str()) {
            continue;
        }

        for value in values {
            let value = value.to_string_lossy();
            let mut new_value = value.to_owned();

            if arg == "--extern" {
                let rmeta_or_rlib_extension = if value.ends_with(RMETA_FILE_EXTENSION) {
                    Some(RMETA_FILE_EXTENSION)
                } else if value.ends_with(RLIB_FILE_EXTENSION) {
                    Some(RLIB_FILE_EXTENSION)
                } else if value.ends_with(DYLIB_FILE_EXTENSION) {
                    Some(DYLIB_FILE_EXTENSION)
                } else if value == "proc_macro" {
                    None
                } else {
                    // println!("Skipping non-rlib or non-dylib --extern value: {:?}", value);
                    bail!(
                        "Unsupported --extern arg value {:?}. \
                        We only support '.rlib', '.rmeta', or '.so' files",
                        value
                    );
                };

                if let Some(_extension) = rmeta_or_rlib_extension {
                    let (extern_crate_name, crate_rmeta_path) = value
                        .find('=')
                        .map(|idx| value.split_at(idx))
                        .map(|(name, path)| (name, &path[1..]))
                        .with_context(|| {
                            format!(
                                "Failed to parse value of --extern arg as CRATENAME=PATH: {:?}",
                                value
                            )
                        })?;
                    print!(
                        "Found --extern arg, {:?} --> {:?}",
                        extern_crate_name, crate_rmeta_path
                    );
                    let crate_rmeta_path = Path::new(crate_rmeta_path);
                    let crate_name_with_hash =
                        get_crate_name_with_hash_from_path(crate_rmeta_path)?
                            .strip_prefix(RMETA_RLIB_FILE_PREFIX)
                            .with_context(|| {
                                format!(
                                    "Found .rlib or .rmeta file in out_dir that \
                                    didn't start with 'lib' prefix: {}",
                                    crate_rmeta_path.display()
                                )
                            })?;
                    let extern_crate =
                        compile_db
                            .get_crate(crate_name_with_hash)
                            .unwrap_or_else(|| {
                                panic!(
                                    "Found no information about crate: {:?}",
                                    crate_name_with_hash
                                )
                            });
                    println!(" ({:?})", extern_crate);

                    args_or_deps_changed |= extern_crate
                        .is_recreated
                        .expect("field `is_recreated` not properly initialized");
                    let candidates = compile_db.find_compatible_crates_in_prebuilt(&extern_crate);
                    if candidates.len() > 1 {
                        println!(
                            "WARNING: found multiple candidates: {:?}, using the first one",
                            candidates
                        );
                    }
                    if !candidates.is_empty() {
                        let prebuilt_crate = candidates.first().unwrap();
                        println!(
                            "#### Replacing crate {:?} with prebuilt crate at {} ({:?})",
                            extern_crate_name,
                            prebuilt_crate.path.display(),
                            prebuilt_crate,
                        );
                        new_value =
                            format!("{}={}", extern_crate_name, prebuilt_crate.path.display())
                                .into();
                        args_or_deps_changed = true;

                        dependencies.push((
                            prebuilt_crate.path.display().to_string(),
                            prebuilt_crate.available.clone(),
                        ));
                    } else {
                        dependencies.push((
                            extern_crate.path.display().to_string(),
                            extern_crate.available.clone(),
                        ));
                    }
                }
            } else if arg == "-L" {
                let (kind, _path) = value
                    .as_ref()
                    .find('=')
                    .map(|idx| value.split_at(idx))
                    .map(|(kind, path)| (kind, &path[1..])) // ignore the '=' delimiter
                    .with_context(|| {
                        format!("Failed to parse value of -L arg as KIND=PATH: {:?}", value)
                    })?;
                // println!("Found -L arg, {:?} --> {:?}", kind, _path);
                if !(kind == "dependency" || kind == "native") {
                    println!("WARNING: Unsupported -L arg value {:?}. We only support 'dependency=PATH' or 'native=PATH'.", value);
                }
                // TODO: if we need to actually modify any -L argument values, then set `new_value` accordingly here.
            }

            if value != new_value.as_ref() {
                args_or_deps_changed = true;
            }
            recreated_cmd.arg(arg.as_str());
            recreated_cmd.arg(new_value.as_ref());
        }
    }

    if c.name == "phoenix_common" {
        panic!(
            "phoenix_common will be rebuilt, this is usually not an expected behavior. \
            Please check the compile log and tune dependencies if necessary."
        );
    }

    // If any args actually changed, we need to run the re-created command.
    compile_db.mark_recreated(&c, args_or_deps_changed);
    if args_or_deps_changed {
        // Add our directory of prebuilt crates as a library search path, for dependency resolution.
        // This is okay because we removed all of the potentially conflicting crates from the local target/ directory,
        // which ensures that adding in the directory of prebuilt crate .rmeta/.rlib files won't cause rustc to complain
        // about multiple "potentially newer" versions of a given crate.
        recreated_cmd.arg("-L").arg(prebuilt_dir);
        // We also need to add the directory of host dependencies, e.g., proc macro crates and such.
        // recreated_cmd.arg("-L").arg(host_deps_dir_path);

        for env in shlex::split(rustc_env_vars).unwrap() {
            let (k, v) = env.split_once('=').unwrap();
            recreated_cmd.env(k, v);
        }
        // println!("\n\n--------------- Inherited Environment Variables ----------------\n");
        // let _env_cmd = Command::new("env").spawn().unwrap().wait().unwrap();
        println!(
            "About to execute recreated_cmd that had changed arguments or updated dependencies:\n{:?}",
            recreated_cmd
        );
    } else {
        // set_file_mtime(, FileTime::now()).unwrap();
        println!(
            "### Args did not change, skipping recreated_cmd:\n{:?}",
            recreated_cmd
        );
        c.available.make_available();
        return Ok(None);
    }

    // Ensure we have the RUST_TARGET_PATH env var so that rustc can find our target spec JSON file.
    // recreated_cmd.env("RUST_TARGET_PATH", target_dir_path);

    Ok(Some(RustcTask {
        recreated_cmd,
        c: c.clone(),
        dependencies,
    }))
}

fn copy_result(c: &Crate) -> anyhow::Result<()> {
    // copy the library
    let destdir = c.path.parent().unwrap().parent().unwrap();
    let (result_name, is_binary) = if let Some(ext) = c.path.extension() {
        // lib
        (format!("lib{}.{}", c.name, ext.to_string_lossy()), false)
    } else {
        // binary
        (c.name.clone(), true)
    };

    let to = destdir.join(result_name);
    println!("Copy {} to {}", c.path.display(), to.display());
    fs::copy(&c.path, to)?;

    // copy the dep file
    let from = c
        .path
        .with_file_name(format!("{}-{}.d", c.name, c.metadata));
    let to = if !is_binary {
        destdir.join(format!("lib{}.d", c.name))
    } else {
        destdir.join(format!("{}.d", c.name))
    };
    println!("Copy {} to {}", from.display(), to.display());
    fs::copy(from, to)?;

    Ok(())
}

/// Iterates over the contents of the given directory to find crates within it.
///
/// This directory should contain one .rmeta and .rlib file per crate,
/// and those files are named as such:
/// `"lib<crate_name>-<hash>.[rmeta]"`
///
/// This function only looks at the `.rmeta` files in the given directory
/// and extracts from that file name the name of the crate name as a String.
///
/// Returns the set of discovered crates as a map, in which the key is the simple crate name
/// ("my_crate") and the value is the full crate name with the hash included ("my_crate-43462c60d48a531a").
/// The value can be used to define the path to crate's actual .rmeta/.rlib file.
#[allow(unused)]
fn populate_crates_from_dir<P: AsRef<Path>>(dir_path: P) -> io::Result<HashMap<String, String>> {
    let mut crates = HashMap::default();

    // let dir_iter = WalkDir::new(dir_path)
    //     .into_iter()
    //     .filter_map(|res| res.ok());
    let dir_iter = fs::read_dir(dir_path)?
        .into_iter()
        .filter_map(|res| res.ok());

    for entry in dir_iter {
        if !entry.file_type().unwrap().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|p| p.to_str()) == Some(RMETA_FILE_EXTENSION) {
            let filestem = path
                .file_stem()
                .expect("no valid file stem")
                .to_string_lossy();
            if filestem.starts_with("lib") {
                let crate_name_with_hash = &filestem[PREFIX_END..];
                let crate_name_without_hash = crate_name_with_hash.split('-').next().unwrap();
                crates.insert(
                    crate_name_without_hash.to_string(),
                    crate_name_with_hash.to_string(),
                );
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "File {:?} is an .rmeta file that does not begin with 'lib' as expected.",
                        path
                    ),
                ));
            }
        }
    }

    Ok(crates)
}

#[allow(unused)]
fn populate_crates_from_dep_file<P: AsRef<Path>>(
    dep_path: P,
) -> io::Result<HashMap<String, String>> {
    // Parse dependency closure
    let content = fs::read_to_string(dep_path)?;
    let mut all_deps = Vec::new();
    for line in content.lines() {
        // name:[ dep]*
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((name, _deps)) = line.split_once(':') else {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Dep file does not have a valid format",
            ));
        };
        dbg!(&name);
        let path = PathBuf::from(name);
        let filestem = path
            .file_stem()
            .expect("no valid file stem")
            .to_string_lossy();
        if filestem.starts_with(RMETA_RLIB_FILE_PREFIX) {
            all_deps.push(name.to_owned());
        }
    }
    // deduplicate
    all_deps.sort();
    all_deps.dedup();
    // filter out .rs, .d, keep .rlib, .so and transform .rmeta to .rlib
    let mut all_deps: Vec<String> = all_deps
        .into_iter()
        .map(|x| {
            x.strip_suffix(".rmeta")
                .map_or(x.clone(), |y| y.to_owned() + ".rlib")
        })
        .collect();
    all_deps.retain(|x| x.ends_with(".rlib") || x.ends_with(".so"));

    let mut crates = HashMap::default();
    for dep in all_deps {
        let path = PathBuf::from(dep);
        if path.extension().and_then(|p| p.to_str()) == Some(RLIB_FILE_EXTENSION) {
            let filestem = path
                .file_stem()
                .expect("no valid file stem")
                .to_string_lossy();
            if filestem.starts_with("lib") {
                let crate_name_with_hash = &filestem[PREFIX_END..];
                let crate_name_without_hash = crate_name_with_hash.split('-').next().unwrap();
                crates.insert(
                    crate_name_without_hash.to_string(),
                    crate_name_with_hash.to_string(),
                );
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "File {:?} is an .rlib file that does not begin with 'lib' as expected.",
                        path
                    ),
                ));
            }
        }
    }

    Ok(crates)
}

/// Parses the given `path` to obtain the part of the filename before the crate name delimiter '-'.
#[allow(unused)]
fn get_plain_crate_name_from_path<'p>(path: &'p Path) -> anyhow::Result<&'p str> {
    path.file_stem()
        .and_then(|os_str| os_str.to_str())
        .with_context(|| {
            format!(
                "Couldn't get file name of file in out_dir: {}",
                path.display()
            )
        })?
        .split('-')
        .next()
        .with_context(|| {
            format!(
                "File in out_dir missing delimiter '-' between crate name and hash. {}",
                path.display()
            )
        })
}

/// Parses the given `path` to obtain the part of the filename with a hash suffix
fn get_crate_name_with_hash_from_path<'p>(path: &'p Path) -> anyhow::Result<&'p str> {
    path.file_stem()
        .and_then(|os_str| os_str.to_str())
        .with_context(|| {
            format!(
                "Couldn't get file name of file in out_dir: {}",
                path.display()
            )
        })?
        .split('.')
        .next()
        .with_context(|| {
            format!(
                "File in out_dir missing delimiter '.' between crate name and suffix. {}",
                path.display()
            )
        })
}

/// Counts the level of verbosity specified by arguments into `cargo`.
fn count_verbose_arg<'i, S: AsRef<str> + 'i, I: IntoIterator<Item = &'i S>>(args: I) -> usize {
    let mut count = 0;
    for arg in args
        .into_iter()
        .flat_map(|a| shlex::split(a.as_ref()).unwrap())
    {
        count += match arg.as_ref() {
            "--verbose" | "-v" => 1,
            "-vv" => 2,
            _ => 0,
        };
    }
    count
}

/// Parse the given verbose rustc command string and return the value of the "--out-dir" argument.
fn get_out_dir_arg(cmd_str: &str) -> anyhow::Result<String> {
    let out_dir_str_start = cmd_str
        .find(" --out-dir")
        .map(|idx| &cmd_str[idx..])
        .context("Captured rustc command did not have an --out-dir argument")?;
    let out_dir_parse = rustc_clap_options("")
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .allow_external_subcommands(true)
        .no_binary_name(true)
        .color(clap::ColorChoice::Never)
        .try_get_matches_from(shlex::split(out_dir_str_start).unwrap());
    let matches =
        out_dir_parse.context("Could not parse --out-dir argument in captured rustc command.")?;
    matches
        .get_one::<String>("--out-dir")
        .cloned()
        .context("--out-dir argument did not have a value")
}

#[allow(unused)]
fn remove_fingerprint_directory<P: AsRef<Path>>(fingerprint_dir: P) -> anyhow::Result<()> {
    let fingerprint_dir_path = fingerprint_dir.as_ref();
    println!(
        "--> Removing .fingerprint directory: {}",
        fingerprint_dir_path.display()
    );
    fs::remove_dir_all(&fingerprint_dir_path).with_context(|| {
        format!(
            "Failed to remove .fingerprint directory: {}",
            fingerprint_dir_path.display(),
        )
    })?;
    Ok(())
}

/// Move cargo's 1.fingerprint` directory temporarily.
///
/// Rename it back when this guard is dropped.
///
/// If the `.fingerprint` does not exist at all, it will return an error.
struct FingerprintDirGuard {
    target_dir: PathBuf,
}

impl FingerprintDirGuard {
    fn new<P: AsRef<Path>>(target_dir: P, profile_dir: &str) -> anyhow::Result<Self> {
        let target_dir = target_dir.as_ref().to_path_buf().join(profile_dir);
        let fingerprint_dir = target_dir.join(".fingerprint");
        let renamed_fingerprint_dir = target_dir.join(".fingerprint.orig");
        Self::move_dir(&fingerprint_dir, &renamed_fingerprint_dir)?;
        Ok(Self { target_dir })
    }

    fn move_dir(from: &Path, to: &Path) -> anyhow::Result<()> {
        println!(
            "--> Moving .fingerprint directory from {} to {}",
            from.display(),
            to.display(),
        );
        if to.exists() && to.is_dir() {
            fs::remove_dir_all(&to).with_context(|| {
                format!("Failed to remove .fingerprint directory: {}", to.display())
            })?;
        }
        let result = fs::rename(from, to);
        match result {
            Ok(_) => Ok(()),
            Err(err) => {
                // If the error is `Invalid cross-device link (os error 18)`, we fallback to copy and delete
                if let Some(18) = err.raw_os_error() {
                    Self::copy_dir_recursively(from, to).with_context(|| {
                        format!(
                            "Failed to copy .fingerprint directory recursively from {} to {}",
                            from.display(),
                            to.display(),
                        )
                    })?;
                    fs::remove_dir_all(&from).with_context(|| {
                        format!(
                            "Failed to remove .fingerprint directory: {}",
                            from.display(),
                        )
                    })?;
                    Ok(())
                } else {
                    Err(err).with_context(|| {
                        format!(
                            "Failed to move .fingerprint directory from {} to {}",
                            from.display(),
                            to.display(),
                        )
                    })
                }
            }
        }
    }

    fn copy_dir_recursively(source: &Path, destination: &Path) -> anyhow::Result<()> {
        fs::create_dir_all(&destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let filetype = entry.file_type()?;
            if filetype.is_dir() {
                Self::copy_dir_recursively(&entry.path(), &destination.join(entry.file_name()))?;
            } else {
                fs::copy(entry.path(), destination.join(entry.file_name()))?;
            }
        }
        Ok(())
    }
}

/// Recovers the fingerprint directory.
///
/// If phoenix_cargo is interrupted or quits during the second pass, the temporarily
/// fingerprint directory `.fingerprint.orig` is present on the file system and that one should
/// be the one in use.
fn recover_fingerprint_directory<P: AsRef<Path>>(
    target_dir: P,
    profile_dir: &str,
) -> anyhow::Result<()> {
    let target_dir = target_dir.as_ref().join(profile_dir);
    let fingerprint_dir = target_dir.join(".fingerprint");
    let renamed_fingerprint_dir = target_dir.join(".fingerprint.orig");
    if renamed_fingerprint_dir.exists() {
        FingerprintDirGuard::move_dir(&renamed_fingerprint_dir, &fingerprint_dir)?;
    }
    Ok(())
}

impl Drop for FingerprintDirGuard {
    fn drop(&mut self) {
        let fingerprint_dir = self.target_dir.join(".fingerprint");
        let renamed_fingerprint_dir = self.target_dir.join(".fingerprint.orig");
        Self::move_dir(&renamed_fingerprint_dir, &fingerprint_dir)
            .expect("Failed to fingerprint directory back");
    }
}

fn remove_redundant_artifacts<P: AsRef<Path>>(
    compile_db: &CompileDb,
    out_dir: P,
) -> anyhow::Result<()> {
    for entry in fs::read_dir(&out_dir)? {
        let entry = entry.unwrap();
        let path = entry.path();
        if entry.file_type().unwrap().is_dir() {
            println!(
                "Found unexpected directory entry in out_dir: {}",
                path.display()
            );
            continue;
        }
        if !entry.file_type().unwrap().is_file() {
            bail!(
                "Found unexpected non-file entry in out_dir: {}",
                path.display()
            );
        }

        // We should remove all potential redundant files, including:
        // * <crate_name>-<hash>.o
        // * lib<crate_name>-<hash>.rmeta
        // * lib<crate_name>-<hash>.rlib
        // * lib<crate_name>-<hash>.so
        //
        // DO NOT remove * <crate_name>-<hash>.d
        //
        // We do not know the exact hash value appended to each crate, we only know the plain crate name.
        // Here, extract the plain crate_name from the file name.
        let crate_name_with_hash = match path.extension().and_then(|os_str| os_str.to_str()) {
            Some("d") | Some("o") => get_crate_name_with_hash_from_path(&path)?,
            Some(RMETA_FILE_EXTENSION) | Some(RLIB_FILE_EXTENSION) | Some(DYLIB_FILE_EXTENSION) => {
                let libcrate_name = get_crate_name_with_hash_from_path(&path)?;
                if libcrate_name.starts_with(RMETA_RLIB_FILE_PREFIX) {
                    &libcrate_name[PREFIX_END..]
                } else {
                    bail!("Found .rlib or .rmeta file in out_dir that didn't start with 'lib' prefix: {}", path.display());
                }
            }
            _ => {
                println!(
                    "Removing potentially-redundant file with unexpected extension: {}",
                    path.display()
                );
                fs::remove_file(&path).with_context(|| {
                    format!(
                        "Failed to remove potentially-redundant file with unexpected extension: {}",
                        path.display()
                    )
                })?;
                continue;
            }
        };

        // See if that crate already exists in our set of prebuilt crates.
        if compile_db
            .get_crate(crate_name_with_hash)
            .map(|c| !compile_db.find_compatible_crates_in_prebuilt(&c).is_empty())
            == Some(true)
        {
            // remove the redundant file
            println!("### Removing redundant crate file {}", path.display());
            fs::remove_file(&path).with_context(|| {
                format!(
                    "Failed to remove redundant crate file in out_dir: {}",
                    path.display(),
                )
            })?;
        } else {
            // Here, do nothing. We must keep the non-redundant files,
            // as they represent new dependencies that were not part of
            // the original in-tree Theseus build.
        }
    }

    Ok(())
}

/// Creates a `Clap::App` instance that handles all (most) of the command-line arguments
/// accepted by the `rustc` executable.
///
/// I obtained this by looking at the output of `rustc --help --verbose`.
fn rustc_clap_options(app_name: &'static str) -> clap::Command {
    clap::Command::new(app_name)
        // The first argument that we want to see, --crate-name.
        .arg(
            clap::Arg::new("--crate-name")
                .long("crate-name")
                .num_args(1),
        )
        // Note: add any other arguments that you encounter in a rustc invocation here.
        .arg(
            clap::Arg::new("-L")
                .short('L')
                .num_args(1)
                .action(clap::ArgAction::Append),
        )
        .arg(
            clap::Arg::new("-l")
                .short('l')
                .num_args(1)
                .action(clap::ArgAction::Append),
        )
        .arg(
            clap::Arg::new("--extern")
                .long("extern")
                .num_args(1)
                .action(clap::ArgAction::Append),
        )
        .arg(
            clap::Arg::new("-C")
                .short('C')
                .long("codegen")
                .num_args(1)
                .action(clap::ArgAction::Append),
        )
        .arg(
            clap::Arg::new("-W")
                .short('W')
                .long("warn")
                .num_args(1)
                .action(clap::ArgAction::Append),
        )
        .arg(
            clap::Arg::new("-A")
                .short('A')
                .long("allow")
                .num_args(1)
                .action(clap::ArgAction::Append),
        )
        .arg(
            clap::Arg::new("-D")
                .short('D')
                .long("deny")
                .num_args(1)
                .action(clap::ArgAction::Append),
        )
        .arg(
            clap::Arg::new("-F")
                .short('F')
                .long("forbid")
                .num_args(1)
                .action(clap::ArgAction::Append),
        )
        .arg(
            clap::Arg::new("--cap-lints")
                .long("cap-lints")
                .num_args(1)
                .action(clap::ArgAction::Append),
        )
        .arg(
            clap::Arg::new("-Z")
                .short('Z')
                .num_args(1)
                .action(clap::ArgAction::Append),
        )
        .arg(
            clap::Arg::new("--crate-type")
                .long("crate-type")
                .num_args(1)
                .action(clap::ArgAction::Append),
        )
        .arg(
            clap::Arg::new("--emit")
                .long("emit")
                .num_args(1)
                .action(clap::ArgAction::Append),
        )
        .arg(clap::Arg::new("--edition").long("edition").num_args(1))
        .arg(clap::Arg::new("-g").short('g'))
        .arg(clap::Arg::new("-O").short('O'))
        .arg(clap::Arg::new("--out-dir").long("out-dir").num_args(1))
        .arg(
            clap::Arg::new("--error-format")
                .long("error-format")
                .num_args(1),
        )
        .arg(clap::Arg::new("--json").long("json").num_args(1))
        .arg(clap::Arg::new("--target").long("target").num_args(1))
        .arg(clap::Arg::new("--sysroot").long("sysroot").num_args(1))
        .arg(clap::Arg::new("--edition").long("edition").num_args(1))
        .arg(
            clap::Arg::new("--cfg")
                .long("cfg")
                .num_args(1)
                .action(clap::ArgAction::Append),
        )
        .arg(
            clap::Arg::new("--verbose")
                .short('v')
                .long("verbose")
                .num_args(0)
                .action(clap::ArgAction::Append),
        )
        .arg(
            clap::Arg::new("--remap-path-prefix")
                .long("remap-path-prefix")
                .num_args(0)
                .action(clap::ArgAction::Append),
        )
}
