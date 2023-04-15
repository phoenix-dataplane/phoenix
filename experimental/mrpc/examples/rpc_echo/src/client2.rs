pub mod rpc_hello {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_hello");
    // include!("../../../mrpc/src/codegen.rs");
}

use rpc_hello::greeter_client::GreeterClient;
use rpc_hello::HelloRequest;
use std::{thread, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = GreeterClient::connect("localhost:5000")?;
    let mut i = 0;
    loop {
        i = i ^ 1;
        let mut req = HelloRequest {
            name: "mRPC".into(),
        };
        if i == 0 {
            req = HelloRequest {
                name: "nRPC".into(),
            };
        }
        println!("send: {:?}", req.name);
        let reply = smol::block_on(client.say_hello(req));
        match reply {
            Ok(reply) => {
                println!("reply: {}", String::from_utf8_lossy(&reply.message));
            }
            Err(e) => {
                println!("error: {}", e);
            }
        }
        thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}
