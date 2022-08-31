use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use thiserror::Error;

use super::{MethodIdentifier, RpcMethodInfo};

mod code_generator;

const MRPC_DERIVE: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mrpc-derive");
const MRPC_MARSHAL: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../mrpc-marshal");
const SHM: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../shm");
const INTERFACE: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../interface");
const TOOLCHAIN: &'static str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../rust-toolchain"
));

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO Error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Syn Parse Error: {0}")]
    SynParse(#[from] syn::Error),
    #[error("Cargo build failed")]
    Cargo,
}

#[cfg(target_os = "macos")]
pub const DYLIB_FILENAME: &'static str = "libdispatch.dylib";
#[cfg(target_os = "linux")]
pub const DYLIB_FILENAME: &'static str = "libdispatch.so";

pub struct Builder {
    /// directory to emit the dispatch dylib crate
    pub(crate) emit_crate_dir: PathBuf,
    /// the directory to the generated Rust code produeced by prost-build
    /// default to "prost" in emit_cache_dir's parent directory
    pub(crate) prost_out_dir: Option<PathBuf>,
    /// the name of the include file generated by prost-build
    /// default to "_include.rs"
    pub(crate) include_filename: Option<String>,
    pub(crate) method_type_mapping: HashMap<MethodIdentifier, RpcMethodInfo>,
}

impl Builder {
    pub fn compile(&self) -> Result<PathBuf, Error> {
        if self.emit_crate_dir.is_dir() {
            fs::remove_dir_all(&self.emit_crate_dir).unwrap();
        } else if self.emit_crate_dir.is_file() {
            fs::remove_file(&self.emit_crate_dir).unwrap();
        }
        fs::create_dir(&self.emit_crate_dir).unwrap();
        fs::create_dir(&self.emit_crate_dir.join("src")).unwrap();

        let prost_out_dir = self
            .prost_out_dir
            .to_owned()
            .unwrap_or(self.emit_crate_dir.parent().unwrap().join("prost"));
        let prost_include_file = prost_out_dir
            .join(
                self.include_filename
                    .as_ref()
                    .map(|x| x.as_str())
                    .unwrap_or("_include.rs"),
            )
            .canonicalize()
            .unwrap();

        let tokens = code_generator::generate(prost_include_file, &self.method_type_mapping)?;

        let ast: syn::File = syn::parse2(tokens)?;
        let code = prettyplease::unparse(&ast);
        fs::write(self.emit_crate_dir.join("src/mod"), &code).unwrap();

        let manifest = format!(
            include_str!("template/Cargo.toml"),
            mrpc_derive = MRPC_DERIVE,
            mrpc_marshal = MRPC_MARSHAL,
            shm = SHM,
            interface = INTERFACE
        );
        fs::write(self.emit_crate_dir.join("Cargo.toml"), manifest).unwrap();

        fs::write(self.emit_crate_dir.join("rust-toolchain"), TOOLCHAIN).unwrap();

        let mut cmd = Command::new("cargo");
        cmd.arg("build").arg("--release");
        cmd.current_dir(&self.emit_crate_dir);

        let status = cmd.status()?;
        if !status.success() {
            // failed to run cargo build
            return Err(Error::Cargo);
        }

        let dylib_path = self
            .emit_crate_dir
            .join(format!("target/release/{}", DYLIB_FILENAME));

        Ok(dylib_path)
    }
}
