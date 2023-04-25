use std::env::var;

fn main() {
    let result = match var("CLIENT_SERVER") {
        Ok(v) => v,
        Err(_) => todo!(),
    };
    if result == "client" {
        cxx_build::bridge("src/clientcodegen.rs") // returns a cc::Build
            .file("src/client.cc")
            .flag_if_supported("-std=c++11")
            .flag_if_supported("-g")
            .compile("cpp_client");
    } else if result == "server" {
        cxx_build::bridge("src/servercodegen.rs") // returns a cc::Build
            .file("src/server.cc")
            .file("src/incrementasync.cc")
            .flag_if_supported("-std=c++11")
            .compile("cpp_server");
    } else {
        println!("warning={}", result);
    }
}
