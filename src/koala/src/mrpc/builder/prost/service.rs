use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use itertools::Itertools;

use crate::mrpc::builder::{MethodIdentifier, RpcMethodInfo};


pub struct ServiceRecorder {
    pub mapping: Rc<RefCell<HashMap<MethodIdentifier, RpcMethodInfo>>>,
    pub compile_well_known_types: bool,
}

impl prost_build::ServiceGenerator for ServiceRecorder {
    fn generate(&mut self, service: prost_build::Service, _buf: &mut String) {
        let mut mapping = self.mapping.borrow_mut();
        let package = &service.package[..];
        let service_path = get_service_path(package, &service);
        let service_id = get_mrpc_service_id(&service_path);
        for method in service.methods.iter() {
            let method_path = get_method_path(package, &service, &method);
            let func_id = get_mrpc_func_id(&method_path);
            let input_type_canonical = resolve_ident(
                package, 
                &method.input_proto_type,
                &method.input_type,
                self.compile_well_known_types
            );
            let output_type_canonical = resolve_ident(
                package,
                &method.input_proto_type,
                &method.input_type,
                self.compile_well_known_types
            );
            let method_info = RpcMethodInfo {
                service_id,
                func_id,
                input_type: input_type_canonical,
                output_type: output_type_canonical
            };
            let method_id = MethodIdentifier(service_id, func_id);
            mapping.insert(method_id, method_info);
        }
    }
}

const NON_PATH_TYPE_ALLOWLIST: &[&str] = &["()"];

fn is_google_type(ty: &str) -> bool {
    ty.starts_with(".google.protobuf")
}

fn resolve_ident(
    local_path: &str, 
    proto_type: &str, 
    rust_type: &str, 
    compile_well_known_types: bool
) -> String {
    if (is_google_type(proto_type) && !compile_well_known_types)
        || rust_type.starts_with("::")
        || NON_PATH_TYPE_ALLOWLIST.iter().any(|ty| *ty == rust_type)
    {
        // fully qualified path or prost-types
        rust_type.to_owned()
    } else if rust_type.starts_with("crate::") {
        panic!("rust_type should not refer to local crate")
    } else {
        // relative path
        let mut local_path = local_path.split('.');
        let mut rust_type_path = rust_type.split("::").peekable();
        while rust_type_path.peek() == Some(&"super") {
            rust_type_path.next();
            local_path.next_back();
        }
        local_path.chain(rust_type_path).join("::")
    }
}

// Returns a fully qualified path of a service
fn get_service_path(package: &str, service: &prost_build::Service) -> String {
    format!(
        "{}{}{}",
        package,
        if package.is_empty() { "" } else { "." },
        service.proto_name,
    )
}

// Returnsa fully qualified path of a method
fn get_method_path(
    package: &str,
    service: &prost_build::Service,
    method: &prost_build::Method,
) -> String {
    format!(
        "/{}{}{}/{}",
        package,
        if package.is_empty() { "" } else { "." },
        service.proto_name,
        method.proto_name,
    )
}

// Calculate SERVICE_ID for mRPC NamedService.
fn get_mrpc_service_id(path: &str) -> u32 {
    crc32fast::hash(path.as_bytes())
}

// Calculate FUNC_ID for mRPC remote procedures.
fn get_mrpc_func_id(path: &str) -> u32 {
    crc32fast::hash(path.as_bytes())
}