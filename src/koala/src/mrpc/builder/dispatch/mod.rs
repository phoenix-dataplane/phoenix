use std::path::PathBuf;
use std::process::Command;

mod code_generator;

pub struct Builder {
    emit_crate_dir: PathBuf,
    prost_out_dir: Option<PathBuf>,
    include_filename: Option<PathBuf>,
}

impl Builder {
    pub fn compile() {
        let mut cmd = Command::new("cargo");
        cmd.args("--include");
    }
}