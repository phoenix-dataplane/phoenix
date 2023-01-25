static CLIENT_SERVER: &str = "client";

fn main() {
    if CLIENT_SERVER == "client" {
        cxx_build::bridge("src/main.rs") // returns a cc::Build
            .file("src/client.cc")
            .file("src/incrementerclient.cc")
            .flag_if_supported("-std=c++11")
            .compile("cpp_client");
    } else if CLIENT_SERVER  == "server" {
        cxx_build::bridge("src/main.rs") // returns a cc::Build
            .file("src/server.cc")
            .file("src/increment.cc")
            .flag_if_supported("-std=c++11")
            .compile("cpp_server");
    } else {
        println!("warning={}", CLIENT_SERVER);
    } 


    println!("cargo:rerun-if-changed=src/main.rs");
    println!("cargo:rerun-if-changed=src/server.cc");
    println!("cargo:rerun-if-changed=src/incrementerclient.cc");
    println!("cargo:rerun-if-changed=src/client.cc");
    println!("cargo:rerun-if-changed=include/incrementerclient.h");
}
