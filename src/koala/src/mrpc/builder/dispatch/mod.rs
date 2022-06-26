use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use super::{MethodIdentifier, RpcMethodInfo};

mod code_generator;

const MRPC_DERIVE: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/../mrpc-derive");
const MRPC_MARSHAL: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/../mrpc-derive");
const IPC: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/../ipc");
const INTERFACE: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/../interface");
const TOOLCHAIN: &'static str = include_str!("../../../../../../rust-toolchain");

pub struct Builder {
    emit_crate_dir: PathBuf,
    prost_out_dir: Option<PathBuf>,
    include_filename: Option<String>,
    method_type_mapping: HashMap<MethodIdentifier, RpcMethodInfo>
}

impl Builder {
    pub fn compile(&self) {
        if self.emit_crate_dir.is_dir() {
            std::fs::remove_dir_all(&self.emit_crate_dir).unwrap();
        } else if self.emit_crate_dir.is_file() {
            std::fs::remove_file(&self.emit_crate_dir).unwrap();
        }
        std::fs::create_dir(&self.emit_crate_dir).unwrap();
        std::fs::create_dir(&self.emit_crate_dir.join("src")).unwrap();

        let prost_out_dir = self.prost_out_dir.to_owned().unwrap_or(
            self.emit_crate_dir.parent().unwrap().join("prost")
        );
        let prost_include_file = prost_out_dir.join(
            self.include_filename.as_ref().map(|x| x.as_str()).unwrap_or("_include.rs")
        ).canonicalize().unwrap();

        let tokens = code_generator::generate(prost_include_file, &self.method_type_mapping);
        let ast: syn::File = syn::parse2(tokens).expect("not a valid tokenstream");
        let code = prettyplease::unparse(&ast);
        std::fs::write(self.emit_crate_dir.join("src/lib.rs"), &code).unwrap();

        let manifest = format!(
            include_str!("template/Cargo.toml"),
            mrpc_derive=MRPC_DERIVE,
            mrpc_marshal=MRPC_MARSHAL,
            ipc=IPC,
            interface=INTERFACE
        );
        std::fs::write(self.emit_crate_dir.join("Cargo.toml"), manifest).unwrap();
        
        std::fs::write(self.emit_crate_dir.join("rust-toolchain"), TOOLCHAIN).unwrap();

        let mut cmd = Command::new("cargo");
        cmd.arg("build")
            .arg("--offline")
            .arg("--release");
        
    }
}