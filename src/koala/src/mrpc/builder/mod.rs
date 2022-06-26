use std::path::PathBuf;

use serde::{Serialize, Deserialize};

pub mod dispatch;
pub mod prost;
pub mod cache;

const PROTO_DIR: &'static str = "proto";


#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct MethodIdentifier(u32, u32);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RpcMethodInfo {
    pub service_id: u32,
    pub func_id: u32,
    // fully qualified path for the method's input rust type
    // for extern path, should start with "::"
    // for local path that generated by prost-build,
    // should start with the package name, e.g., `foo::bar::HelloRequest`
    // instead of starting with "::" qualifier
    pub input_type: String,
    // output type's path
    pub output_type: String    
} 

pub fn build_dispatch_library(
    protos: Vec<String>,
    cache_dir: PathBuf
) {
    let (identifier, cached) = cache::check_cache(&protos, cache_dir, PROTO_DIR);
    if !cached {
        cache::write_protos_to_cache(identifier, &protos, cache_dir, PROTO_DIR);
    }
    
}