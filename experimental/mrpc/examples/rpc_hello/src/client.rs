pub mod rpc_hello {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_hello");
    // include!("../../../mrpc/src/codegen.rs");
}

use rpc_hello::greeter_client::GreeterClient;
use rpc_hello::HelloRequest;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = GreeterClient::connect("localhost:5000")?;
    let req = HelloRequest {
        name: "mRPC".into(),
    };
    let reply = smol::block_on(client.say_hello(req))?;
    println!("reply: {}", String::from_utf8_lossy(&reply.message));
    Ok(())
}
