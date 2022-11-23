fn main() {
    cxx_build::bridge("src/main.rs") // returns a cc::Build
        .file("src/main.cc")
        .flag_if_supported("-std=c++11")
        .compile("ffi");

    println!("cargo:rerun-if-changed=src/main.rs");
    println!("cargo:rerun-if-changed=src/main.cc");
    println!("cargo:rerun-if-changed=include/main.h");
}
