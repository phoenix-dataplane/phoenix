use std::path::PathBuf;

use koala::mrpc::builder::build_dispatch_library;

const PROTO: &'static str = include_str!("../proto/rpc_hello.proto");
const CACHE_DIR: &'static str = "/tmp/KOALA_DISPATCH";

fn main() {
    let protos = vec![PROTO.to_string()];
    build_dispatch_library(protos, PathBuf::from(CACHE_DIR)).unwrap();
}
