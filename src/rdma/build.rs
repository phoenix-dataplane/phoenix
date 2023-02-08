extern crate bindgen;

use std::env;
use std::path::PathBuf;

fn main() {
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());

    println!("You need to have librdmacm and libibverbs installed in your system.");
    println!("cargo:rerun-if-changed=src/rdma_verbs_wrapper.c");

    // let mut cc_command = cc::Build::new()
    //     .warnings(true)
    //     .opt_level(3)
    //     .cargo_metadata(false)
    //     .pic(true)
    //     .use_plt(false)
    //     .shared_flag(true)
    //     .static_flag(true)
    //     .get_compiler()
    //     .to_command();

    // // Compile dynamic library manually as cc-rs is not intended to create dynamic library.
    // cc_command.args([
    //     "src/rdma_verbs_wrapper.c",
    //     "-o",
    //     &out_path.join("librdma_verbs_wrapper.so").to_string_lossy(),
    // ]);

    // // COMMENT(cjr): Remove this comment to see the compiler command.
    // // println!("cargo:warning=compiler command: {:?}", cc_command);
    // cc_command
    //     .spawn()
    //     .expect("Failed to build the shared library");

    cc::Build::new()
        .warnings(true)
        .opt_level(3)
        .cargo_metadata(false)
        .pic(true)
        .use_plt(false)
        .shared_flag(true)
        .static_flag(true)
        .file("src/rdma_verbs_wrapper.c")
        .compile("rdma_verbs_wrapper");

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
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    println!("cargo:rustc-link-search=native={}", out_path.display());
    println!("cargo:rustc-link-lib=static=rdma_verbs_wrapper");
    // println!("cargo:rustc-link-lib=dylib=rdma_verbs_wrapper");
    // println!("cargo:rustc-link-arg=-Wl,-rpath={}", out_path.display());
    // println!(
    //     "cargo:rustc-cdylib-link-arg=-Wl,-rpath={}",
    //     out_path.display()
    // );

    println!("cargo:rustc-link-lib=ibverbs");
    println!("cargo:rustc-link-lib=rdmacm");
}
