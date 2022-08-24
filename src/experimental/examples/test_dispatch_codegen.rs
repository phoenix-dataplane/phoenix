// use std::path::PathBuf;

// use structopt::StructOpt;

// use koala::mrpc::builder::build_serializer_lib;

// const HELLO_PROTO: &'static str = include_str!("../proto/rpc_hello.proto");
// const DEFAULT_CACHE_DIR: &'static str = "/tmp/koala_dispatch";

// #[derive(StructOpt)]
// struct Opt {
//     #[structopt(short = "p", parse(from_os_str))]
//     proto: Option<PathBuf>,
//     #[structopt(short = "c", long = "cache", parse(from_os_str))]
//     cache_dir: Option<PathBuf>,
// }

fn main() {
    // let opt = Opt::from_args();
    // let proto = if let Some(path) = opt.proto {
    //     std::fs::read_to_string(path).unwrap()
    // } else {
    //     HELLO_PROTO.to_string()
    // };
    // let protos = vec![proto];
    // let cache_dir = opt.cache_dir.unwrap_or(DEFAULT_CACHE_DIR.into());
    // build_serializer_lib(protos, cache_dir).unwrap();
}
