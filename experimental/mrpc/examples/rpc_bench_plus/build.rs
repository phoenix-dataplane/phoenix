const PROTO: &str = "../proto/rpc_plus/rpc_plus.proto";
fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed={PROTO}");
    mrpc_build::compile_protos(PROTO)?;
    Ok(())
}
