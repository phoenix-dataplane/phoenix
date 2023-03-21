//! This code defines a simple mRPC client to send a "Hello" request to an mRPC server
//! and prints the response received from the server.

// Import the auto-generated code for the "rpc_hello" module from the Proto file.
pub mod rpc_hello {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_hello");
}

use rpc_hello::greeter_client::GreeterClient;
use rpc_hello::HelloRequest;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a new GreeterClient and connect to the mRPC server at "localhost:5000".
    let client = GreeterClient::connect("localhost:5000")?;

    // Create a new HelloRequest with the name "mRPC".
    let req = HelloRequest {
        name: "mRPC".into(),
    };

    // Send the HelloRequest to the server using the "say_hello" method, and
    // block on the future until the reply is received.
    let reply = smol::block_on(client.say_hello(req))?;

    // Print the reply message and return Ok.
    println!("reply: {}", String::from_utf8_lossy(&reply.message));
    Ok(())
}
