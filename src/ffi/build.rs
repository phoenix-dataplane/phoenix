fn main() {
    cxx_build::bridge("src/main.rs") // returns a cc::Build
        .file("src/client.cc")
        .file("src/incrementerclient.cc")
        .flag_if_supported("-std=c++11")
        .compile("ffi");

    println!("cargo:rerun-if-changed=src/main.rs");
    println!("cargo:rerun-if-changed=src/incrementerclient.cc");
    println!("cargo:rerun-if-changed=src/client.cc");
    println!("cargo:rerun-if-changed=include/incrementerclient.h");
}
