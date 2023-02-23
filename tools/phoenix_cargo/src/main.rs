// Inspired by Theseus cargo.
// https://github.com/theseus-os/Theseus/blob/89489db4a11f2b0ea398d72740a0258111390f5f/tools/theseus_cargo/src/main.rs
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use anyhow::{bail, Context};
use clap::Parser;
use walkdir::WalkDir;

#[derive(Debug, Parser)]
#[command(
    about = "A wrapper around cargo to support out-of-tree build of phoenix plugins \
    based on a previous build of phoenix."
)]
struct Opts {
    /// The sysroot for the rustup toolchains that contains the latest build of phoenix_common
    /// and its dependencies.
    #[arg(long)]
    sysroot: PathBuf,

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

fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();

    let sysroot_dir_path = fs::canonicalize(&opts.sysroot)
        .with_context(|| format!("--input arg '{}' was invalid path.", opts.sysroot.display()))?;

    let prebuilt_crates_set = if sysroot_dir_path.is_dir() {
        populate_crates_from_dir(&sysroot_dir_path)
            .context("Error parsing --input arg as directory.")?
    } else {
        bail!(
            "Couldn't access --input argument '{}' as a directory",
            sysroot_dir_path.display()
        )
    };

    let verbose_count = count_verbose_arg(&opts.cargo_subcommand);

    // dbg!(&opts.cargo_subcommand);

    let stderr_captured =
        run_initial_cargo(&opts.cargo_subcommand, &sysroot_dir_path, verbose_count)?;

    if !is_build_command(&opts.cargo_subcommand) {
        println!("Exiting after completing non-'build' cargo command.");
        return Ok(());
    }

    let last_cmd = stderr_captured
        .last()
        .context("No commands captured from stderr during the initial cargo command")?;
    let out_dir = PathBuf::from(get_out_dir_arg(last_cmd)?);

    // Now that we have run the initial cargo build, it has created many redundant dependency artifacts
    // in the local crate's target/ directory, namely the locally re-built versions of phoenix crates,
    // specifically all the crates that are in the set of prebuilt crates.
    // We need to remove those redundant files from the local target/ directory (the "out-dir")
    // such that when we re-issue the rustc commands below, it won't fail with an error about
    // multiple "potentially newer" versions of a given crate dependency.
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
        //
        // DO NOT remove * <crate_name>-<hash>.d
        //
        // We do not know the exact hash value appended to each crate, we only know the plain crate name.
        // Here, extract the plain crate_name from the file name.
        let crate_name = match path.extension().and_then(|os_str| os_str.to_str()) {
            Some("d") | Some("o") => get_plain_crate_name_from_path(&path)?,
            Some(RMETA_FILE_EXTENSION) | Some(RLIB_FILE_EXTENSION) | Some(DYLIB_FILE_EXTENSION) => {
                let libcrate_name = get_plain_crate_name_from_path(&path)?;
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
        if prebuilt_crates_set.contains_key(crate_name) {
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

    // Here, remove the ".fingerprint/` directory, in order to force rustc
    // to rebuild all artifacts for all of the modified rustc commands that we re-run below.
    // Those fingerprint files are in the actual target directory, which is the parent directory of `out_dir`.
    let target_dir = out_dir.parent().unwrap();
    let mut fingerprint_dir_path = target_dir.to_path_buf();
    fingerprint_dir_path.push(".fingerprint");
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

    // Obtain the directory for the host system dependencies
    // let mut host_deps_dir_path = PathBuf::from(&input_dir_path);
    // host_deps_dir_path.push(&build_config.host_deps);

    // re-execute the rustc commands that we captured from the original cargo verbose output.
    for original_cmd in &stderr_captured {
        // This function will only re-run rustc for crates that don't already exist in the set of prebuilt crates.
        run_rustc_command(original_cmd, &prebuilt_crates_set, &sysroot_dir_path)?;
    }

    Ok(())
}

/// Returns true if the crate is a first party crate.
fn is_first_party_crate(crate_name: &str, _crate_source_file: &str) -> bool {
    // cjr@cjr-office ~/Developing/phoenix (git)-[refactor] % cargo metadata --format-version=1 | jq -c '.packages[] | select(.source == null) | .name'
    [
        // "alltoall",
        // "bench",
        // "benchmark",
        // "experimental",
        // "hello",
        "ipc",
        // "libphoenix",
        "mmap",
        "phoenix-salloc",
        "phoenix-transport-rdma",
        "phoenix-transport-tcp",
        // "phoenix_cargo",
        "phoenix_common",
        // "phoenixctl",
        "phoenixos",
        "rdma",
        // "send_bw",
        // "send_lat",
        // "shm",
        // "shmalloc",
        // "slabmalloc",
        "uapi",
        "uapi-core",
        "uapi-mrpc",
        "uapi-policy-qos",
        "uapi-policy-ratelimit",
        "uapi-rpc-adapter",
        "uapi-salloc",
        "uapi-transport",
        "utils",
    ]
    .contains(&crate_name)
}

// The commands we care about capturing starting with "Running `" and end with "`".
const COMMAND_START: &str = "Running `";
const COMMAND_END: &str = "`";
const RUSTC_CMD_START: &str = "rustc --crate-name";
const BUILD_SCRIPT_CRATE_NAME: &str = "build_script_build";

// The format of rmeta/rlib file names.
const RMETA_RLIB_FILE_PREFIX: &str = "lib";
const RMETA_FILE_EXTENSION: &str = "rmeta";
const RLIB_FILE_EXTENSION: &str = "rlib";
const DYLIB_FILE_EXTENSION: &str = "so";
const PREFIX_END: usize = RMETA_RLIB_FILE_PREFIX.len();

/// Runs the actual cargo build command.
///
/// Returns the captured content of content written to `stderr` by the cargo command, as a list of lines.
fn run_initial_cargo<P: AsRef<Path>>(
    full_args: &[String],
    sysroot_dir_path: P,
    verbose_level: usize,
) -> anyhow::Result<Vec<String>> {
    let sysroot_dir_path = sysroot_dir_path.as_ref();

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

    // TODO: Add the requisite environment variables to configure cargo such that rustc builds with the
    // proper config.
    // cmd.env("RUST_TARGET_PATH");

    // Add the sysroot argument to our rustflags so cargo will use our pre-built phoenix dependencies.
    let mut rustflags = format!("--sysroot {}", sysroot_dir_path.display());

    // -Zbinary-dep-depinfo allows us to track dependencies of each rlib
    rustflags.push_str(" -Zunstable-options -Zbinary-dep-depinfo");
    cmd.env("RUSTFLAGS", rustflags);

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
    let stderr_reader = BufReader::new(stderr);
    let mut stderr_logs = Vec::new();

    // Use regex to strip out the ANSI color codes emitted by the cargo command
    let ansi_escape_regex = regex::Regex::new(r"[\x1B\x9B]\[[^m]+m").unwrap();

    let mut pending_multiline_cmd = false;
    let mut original_multiline = String::new();

    // Capture every line that cargo writes to stderr.
    // We only re-echo the lines that should be outputted by the verbose level specified.
    // The complexity below is due to the fact that a verbose command printed by cargo
    // may span multiple lines, so we need to detect the beginning and end of a multi-line command
    // and merge it into a single line in our captured output.
    stderr_reader
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
                stderr_logs.push(line_stripped.to_string());
                pending_multiline_cmd = !is_final_line;
                original_multiline = String::from(&original_line);
                if !is_final_line {
                    return; // continue to the next line
                }
            } else {
                // Here, we've reached another line, which *may* bethe continuation of
                // a previous rustc command, or it may just be a completely irrelevant
                // line of output.
                if pending_multiline_cmd {
                    // append to the latest line of output instead of adding a new line
                    let last = stderr_logs
                        .last_mut()
                        .expect("BUG: stderr_logs had no last element");
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
            let should_print = |stripped_line: &str| {
                verbose_level > 0 ||  // print everything if verbose
                (
                    // print only "Compiling" and warning/error lines if not verbose
                    !stripped_line.starts_with("+ ")
                    && !stripped_line.starts_with("[")
                    && !stripped_line.starts_with(COMMAND_START)
                )
            };
            if !original_multiline.is_empty() && is_final_line {
                let original_multiline_replaced =
                    ansi_escape_regex.replace_all(&original_multiline, "");
                let original_multiline_stripped = original_multiline_replaced.trim_start();
                if should_print(original_multiline_stripped) {
                    eprintln!("{}", original_multiline)
                }
            } else if should_print(line_stripped) {
                eprintln!("{}", original_line);
            }
        });

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

/// Takes the given `original_cmd` that was captured from the verbose output of cargo,
/// and parses/modifies it to link against (depend on) the corresponding crate of the same name
/// from the list of prebuilt crates.
///
/// The actual dependency files (.rmeta/.rlib) for the prebuilt crates should be located in the `prebuilt_dir`.
/// The target specification JSON file should be found in the `target_dir_path`.
/// These two directories are usually the same directory.
///
/// # Return
/// * Returns `Ok(true` if everything works and the modified rustc command executes properly.
/// * Returns `Ok(false)` if no action needs to be taken.
///   This occurs if `original_cmd` is for building a build script (currently ignored),
///   or if `original_cmd` is for building a crate that already exists in the set of `prebuilt_crates`.
/// * Returns an error if the command fails.
fn run_rustc_command<P: AsRef<Path>>(
    original_cmd: &str,
    prebuilt_crates: &HashMap<String, String>,
    prebuilt_dir: P,
) -> anyhow::Result<bool> {
    let prebuilt_dir = prebuilt_dir.as_ref();

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
        return Ok(false);
    }

    println!("\n\nLooking at original command:\n{}", command);
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

    // Clap will parse the args as such:
    // * the --crate-name will be the first argument
    // * the path to the crate's main file will be the first subcommand
    // * that subcommand's arguments will include ALL OTHER arguments that we care about, specified below.
    let crate_name = top_level_matches
        .get_one::<String>("--crate-name")
        .expect("rustc command did not have required --crate-name argument");
    let (crate_source_file, additional_args) = top_level_matches
        .subcommand()
        .context("Missing crate source files and addition args after rustc")?;

    // Skip build script invocations, as we may not need to re-run those.
    if crate_name == BUILD_SCRIPT_CRATE_NAME {
        println!("\n### Skipping build script build");
        return Ok(false);
    }

    // Skip crates that have already been built. (Not sure if this is always 100% correct)
    if prebuilt_crates.contains_key(crate_name) {
        println!("\n### Skipping already-built crate {:?}", crate_name);
        return Ok(false);
    }

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

    // Now, re-create the rustc command invocation with the proper arguments.
    // First, we handle the --crate-name and --edition arguments, which may come before the crate source file path.
    let mut recreated_cmd = Command::new("rustc");
    recreated_cmd.arg("--crate-name").arg(crate_name);
    if let Some(edition) = top_level_matches.get_one::<String>("--edition") {
        recreated_cmd.arg("--edition").arg(edition);
    }
    recreated_cmd.arg(crate_source_file);

    if !is_first_party_crate(crate_name, crate_source_file) {
        println!("\n### Skipping third-party crate {:?}", crate_name);
        return Ok(false);
    }

    let mut args_changed = false;

    // After adding the initial stuff: rustc command, crate name, (optional --edition), and crate source file,
    // the other arguments are added in the loop below.
    'args_label: for arg in matches.ids() {
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

            if arg == "--crate-type" && value.as_ref() == "proc-macro" {
                // Don't re-run proc_macro builds, as those are built to run on the host.
                args_changed = false;
                break 'args_label;
            } else if arg == "--extern" {
                let rmeta_or_rlib_extension = if value.ends_with(RMETA_FILE_EXTENSION) {
                    Some(RMETA_FILE_EXTENSION)
                } else if value.ends_with(RLIB_FILE_EXTENSION) {
                    Some(RLIB_FILE_EXTENSION)
                } else if value == "proc_macro" {
                    None
                } else if value.ends_with(DYLIB_FILE_EXTENSION) {
                    Some(DYLIB_FILE_EXTENSION)
                } else {
                    println!("Skipping non-rlib --extern value: {:?}", value);
                    None
                    // bail!(
                    //     "Unsupported --extern arg value {:?}. \
                    //     We only support '.rlib' or '.rmeta' files",
                    //     value
                    // );
                };

                if let Some(extension) = rmeta_or_rlib_extension {
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
                    println!(
                        "Found --extern arg, {:?} --> {:?}",
                        extern_crate_name, crate_rmeta_path
                    );
                    if let Some(extern_crate_name_with_hash) =
                        prebuilt_crates.get(extern_crate_name)
                    {
                        let mut new_crate_path = prebuilt_dir.to_path_buf();
                        new_crate_path.push(format!(
                            "{}{}.{}",
                            RMETA_RLIB_FILE_PREFIX, extern_crate_name_with_hash, extension
                        ));
                        println!(
                            "#### Replacing crate {:?} with prebuilt crate at {}",
                            extern_crate_name,
                            new_crate_path.display()
                        );
                        new_value =
                            format!("{}={}", extern_crate_name, new_crate_path.display()).into();
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
                args_changed = true;
            }
            recreated_cmd.arg(arg.as_str());
            recreated_cmd.arg(new_value.as_ref());
        }
    }

    // If any args actually changed, we need to run the re-created command.
    if args_changed {
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
            "About to execute recreated_cmd that had changed arguments:\n{:?}",
            recreated_cmd
        );
    } else {
        println!(
            "### Args did not change, skipping recreated_cmd:\n{:?}",
            recreated_cmd
        );
        return Ok(false);
    }

    // XXX For debugging, uncommment the following
    // println!("Press enter to run the above command ...");
    // let mut buf = String::new();
    // io::stdin().read_line(&mut buf).expect("failed to read stdin");

    // Ensure we have the RUST_TARGET_PATH env var so that rustc can find our target spec JSON file.
    // recreated_cmd.env("RUST_TARGET_PATH", target_dir_path);

    // Finally, we run the recreated rustc command.
    let mut rustc_process = recreated_cmd
        .spawn()
        .context("Failed to run cargo command")?;
    let exit_status = rustc_process.wait().context("Error running rustc")?;

    match exit_status.code() {
        Some(0) => {
            println!("Ran rustc command (modified for Theseus) successfully.");
            Ok(true)
        }
        Some(code) => bail!("rustc command exited with failure code {}", code),
        _ => bail!("rustc command failed and was killed."),
    }
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

/// Parses the given `path` to obtain the part of the filename before the crate name delimiter '-'.
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
