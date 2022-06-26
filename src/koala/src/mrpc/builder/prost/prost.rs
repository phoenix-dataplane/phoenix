use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::ffi::OsString;
use std::rc::Rc;

use crate::mrpc::builder::{MethodIdentifier, RpcMethodInfo};

use super::service::ServiceRecorder;


pub struct Builder {
    file_descriptor_set_path: Option<PathBuf>,
    type_attributes: Vec<(String, String)>,
    field_attributes: Vec<(String, String)>,
    compile_well_known_types: bool,
    extern_paths: Vec<(String, String)>,
    default_package_filename: Option<String>,
    protoc_args: Vec<OsString>,
    include_file: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    method_info_out_path: Option<PathBuf>
}

pub fn configure() -> Builder {
    Builder {
        file_descriptor_set_path: None, 
        type_attributes: Vec::new(),
        field_attributes: Vec::new(),
        compile_well_known_types: false,
        extern_paths: Vec::new(),
        default_package_filename: None,
        protoc_args: Vec::new(),
        include_file: None,
        out_dir: None,
        method_info_out_path: None,
    }
}

pub fn compile_protos(proto: impl AsRef<Path>) -> io::Result<()> {
    let proto_path: &Path = proto.as_ref();

    // directory the main .proto file resides in
    let proto_dir = proto_path
        .parent()
        .expect("proto file should reside in a directory");

    self::configure().compile(&[proto_path], &[proto_dir])?;

    Ok(())
}

impl Builder {
    /// Generate a file containing the encoded `prost_types::FileDescriptorSet` for protocol buffers
    /// modules. This is required for implementing gRPC Server Reflection.
    pub fn file_descriptor_set_path(mut self, path: impl AsRef<Path>) -> Self {
        self.file_descriptor_set_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Add additional attribute to matched messages, enums, and one-offs.
    ///
    /// Passed directly to `prost_build::Config.type_attribute`.
    pub fn type_attribute<P: AsRef<str>, A: AsRef<str>>(mut self, path: P, attribute: A) -> Self {
        self.type_attributes
            .push((path.as_ref().to_string(), attribute.as_ref().to_string()));
        self
    }

    /// Add additional attribute to matched messages, enums, and one-offs.
    ///
    /// Passed directly to `prost_build::Config.field_attribute`.
    pub fn field_attribute<P: AsRef<str>, A: AsRef<str>>(mut self, path: P, attribute: A) -> Self {
        self.field_attributes
            .push((path.as_ref().to_string(), attribute.as_ref().to_string()));
        self
    }

    /// Enable or disable directing Prost to compile well-known protobuf types instead
    /// of using the already-compiled versions available in the `prost-types` crate.
    ///
    /// This defaults to `false`.
    pub fn compile_well_known_types(mut self, compile_well_known_types: bool) -> Self {
        self.compile_well_known_types = compile_well_known_types;
        self
    }

    /// Declare externally provided Protobuf package or type.
    ///
    /// Passed directly to `prost_build::Config.extern_path`.
    /// Note that both the Protobuf path and the rust package paths should both be fully qualified.
    /// i.e. Protobuf paths should start with "." and rust paths should start with "::",
    /// and it should not refer to `crate`
    pub fn extern_path(mut self, proto_path: impl AsRef<str>, rust_path: impl AsRef<str>) -> Self {
        self.extern_paths.push((
            proto_path.as_ref().to_string(),
            rust_path.as_ref().to_string(),
        ));
        self
    }

    /// Configure Prost `protoc_args` build arguments.
    ///
    /// Note: Enabling `--experimental_allow_proto3_optional` requires protobuf >= 3.12.
    pub fn protoc_arg<A: AsRef<str>>(mut self, arg: A) -> Self {
        self.protoc_args.push(arg.as_ref().into());
        self
    }

    /// Configures the optional module filename for easy inclusion of all generated Rust files
    ///
    /// If set, generates a file (inside the `OUT_DIR` or `out_dir()` as appropriate) which contains
    /// a set of `pub mod XXX` statements combining to load all Rust files generated.  This can allow
    /// for a shortcut where multiple related proto files have been compiled together resulting in
    /// a semi-complex set of includes.
    pub fn include_file(mut self, path: impl AsRef<Path>) -> Self {
        self.include_file = Some(path.as_ref().to_path_buf());
        self
    }

    /// Configure the optional 

    /// Compile the .proto files and execute code generation using a
    /// custom `prost_build::Config`.
    /// Returns (service_id, func_id) -> method info mapping
    pub fn compile_with_config(
        self,
        mut config: prost_build::Config,
        protos: &[impl AsRef<Path>],
        includes: &[impl AsRef<Path>],
    ) -> std::io::Result<HashMap<MethodIdentifier, RpcMethodInfo>> {
        let out_dir = if let Some(out_dir) = self.out_dir.as_ref() {
            out_dir.clone()
        } else {
            PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("prost")
        };

        config.out_dir(out_dir);
        if let Some(path) = self.file_descriptor_set_path.as_ref() {
            config.file_descriptor_set_path(path);
        }
        for (proto_path, rust_path) in self.extern_paths.iter() {
            config.extern_path(proto_path, rust_path);
        }
        for (prost_path, attr) in self.field_attributes.iter() {
            config.field_attribute(prost_path, attr);
        }
        for (prost_path, attr) in self.type_attributes.iter() {
            config.type_attribute(prost_path, attr);
        }
        if self.compile_well_known_types {
            config.compile_well_known_types();
        }
        if let Some(path) = self.include_file.as_ref() {
            config.include_file(path);
        }

        for arg in self.protoc_args.iter() {
            config.protoc_arg(arg);
        }

        let method_info = Rc::new(RefCell::new(HashMap::new()));
        let recorder = ServiceRecorder {
            mapping: method_info.clone(),
            compile_well_known_types: self.compile_well_known_types,
        };
        config.service_generator(Box::new(recorder));

        config.compile_protos(protos, includes)?;

        let method_info = Rc::try_unwrap(method_info).unwrap().into_inner();

        if let Some(out_method_info_path) = self.method_info_out_path {
            let json = serde_json::to_string_pretty(&method_info).unwrap();
            std::fs::write(out_method_info_path, json.as_bytes());
        }
        
        Ok(method_info)
    }
}