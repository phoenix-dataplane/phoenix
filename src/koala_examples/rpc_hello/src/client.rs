pub mod rpc_hello {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_hello");
}

use rpc_hello::greeter_client::GreeterClient;
use rpc_hello::HelloRequest;

use mrpc::stub::RpcMessage;

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    let client = GreeterClient::connect("rdma0.danyang-06:5000")?;
    let req = RpcMessage::new(HelloRequest { name: "mRPC".into() });
    let reply = smol::block_on(client.say_hello(&req))?;
    println!("reply: {:?}", String::from_utf8_lossy(&reply.message));
    Ok(())
}
