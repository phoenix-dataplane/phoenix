fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../proto/rpc_hello/rpc_hello.proto");
    println!("cargo:rerun-if-changed=build.rs");
    mrpc_build::compile_protos("../proto/rpc_hello/rpc_hello.proto")?;
    Ok(())
}
