extern crate bindgen;

use std::env;
use std::path::PathBuf;

fn main() {
    println!("You need to have librdmacm and libibverbs installed in your system.");
    println!("cargo:rustc-link-lib=ibverbs");
    println!("cargo:rustc-link-lib=rdmacm");

    cc::Build::new()
        .warnings(true)
        .opt_level(3)
        .file("src/rdma_verbs_wrapper.c")
        .compile("librdma_verbs_wrapper.so");

    cc::Build::new()
        .warnings(true)
        .opt_level(3)
        .file("src/rdma_verbs_wrapper.c")
        .compile("librdma_verbs_wrapper.a");

    println!("cargo:rustc-link-lib=dylib=rdma_verbs_wrapper");

    let bindings = bindgen::Builder::default()
        .header("src/rdma_verbs_wrapper.h")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        .blocklist_type("max_align_t")
        .blocklist_type("ibv_wc")
        .bitfield_enum("ibv_access_flags")
        .bitfield_enum("ibv_qp_attr_mask")
        .bitfield_enum("ibv_wc_flags")
        .bitfield_enum("ibv_send_flags")
        .bitfield_enum("ibv_port_cap_flags")
        .constified_enum_module("ibv_qp_type")
        .constified_enum_module("ibv_qp_state")
        .constified_enum_module("ibv_port_state")
        .constified_enum_module("ibv_wc_opcode")
        .constified_enum_module("ibv_wr_opcode")
        .constified_enum_module("ibv_wc_status")
        .constified_enum_module("rdma_port_space")
        .constified_enum_module("rdma_cm_event_type")
        .derive_default(true)
        .derive_debug(true)
        .generate()
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
