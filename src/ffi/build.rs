use std::env::var;

fn main() {
    let result= match var("CLIENT_SERVER") {
        Ok(v) => v,
        Err(_) => todo!(),
    };
    if result == "client" {
        cxx_build::bridge("src/client.rs") // returns a cc::Build
            .file("src/client.cc")
            .file("src/incrementerclient.cc")
            .flag_if_supported("-std=c++11")
            .compile("cpp_client");
    } else if result  == "server" {
        cxx_build::bridge("src/server.rs") // returns a cc::Build
            .file("src/server.cc")
            .file("src/increment_async.cc")
            .flag_if_supported("-std=c++11")
            .compile("cpp_server");
    } else {
        println!("warning={}", result);
    }

}
