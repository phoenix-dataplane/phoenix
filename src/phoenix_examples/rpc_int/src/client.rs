pub mod rpc_int {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_int");
    // include!("../../../mrpc/src/codegen.rs");
}

use rpc_int::greeter_client::GreeterClient;
use rpc_int::HelloRequest;

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    let client = GreeterClient::connect("127.0.0.1:5000")?;
    let req = HelloRequest {
        val: 0,
    };
    let reply = smol::block_on(client.say_hello(req))?;
    println!("reply: {}", reply.val);
    // should print 1
    Ok(())
}
