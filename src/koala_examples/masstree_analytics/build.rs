use cmake;

const PROTO: &str = "../proto/masstree_analytics/masstree_analytics.proto";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Builds the project in the directory located in `libmt_index`, installing it
    // into $OUT_DIR
    let dst = cmake::build("libmt_index");

    println!("cargo:rustc-link-search=native={}", dst.display());
    println!("cargo:rustc-link-lib=static=mt_index");

    // Generate code for proto
    println!("cargo:rerun-if-changed={PROTO}");
    mrpc_build::compile_protos(PROTO)?;
    Ok(())
}
