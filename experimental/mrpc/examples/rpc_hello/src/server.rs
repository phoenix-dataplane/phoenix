//! This code defines a simple mRPC server that implements the Greeter service.
//! It listens for incoming "Hello" requests and sends back a greeting message.

// Import the auto-generated code for the "rpc_hello" module from the Proto file.
pub mod rpc_hello {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_hello");
}

use rpc_hello::greeter_server::{Greeter, GreeterServer};
use rpc_hello::{HelloReply, HelloRequest};

use mrpc::{RRef, WRef};

#[derive(Debug, Default)]
struct MyGreeter;

// Implement the Greeter trait for MyGreeter using async_trait.
#[mrpc::async_trait]
impl Greeter for MyGreeter {
    // Define the say_hello function which takes an RRef<HelloRequest>
    // and returns a Result with a WRef<HelloReply>.
    async fn say_hello(
        &self,
        request: RRef<HelloRequest>,
    ) -> Result<WRef<HelloReply>, mrpc::Status> {
        // Log the received request.
        eprintln!("request: {:?}", request);

        // Create a new HelloReply with a greeting message.
        let message = format!("Hello {}!", String::from_utf8_lossy(&request.name));
        let reply = WRef::new(HelloReply {
            message: message.as_bytes().into(),
        });

        Ok(reply)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Start the server, binding it to port 5000.
    smol::block_on(async {
        let mut server = mrpc::stub::LocalServer::bind("0.0.0.0:5000")?;

        // Add the Greeter service to the server using the custom MyGreeter implementation.
        server
            .add_service(GreeterServer::new(MyGreeter::default()))
            .serve()
            .await?;
        Ok(())
    })
}
