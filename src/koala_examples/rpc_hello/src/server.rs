pub mod rpc_hello {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_hello");
    // include!("../../../mrpc/src/codegen.rs");
}

use rpc_hello::greeter_server::{Greeter, GreeterServer};
use rpc_hello::{HelloReply, HelloRequest};

use mrpc::{RRef, WRef};

#[derive(Debug, Default)]
struct MyGreeter;

#[mrpc::async_trait]
impl Greeter for MyGreeter {
    async fn say_hello<'s>(
        &self,
        request: RRef<'s, HelloRequest>,
    ) -> Result<WRef<HelloReply>, mrpc::Status> {
        eprintln!("request: {:?}", request);

        let message = format!("Hello {}!", String::from_utf8_lossy(&request.name));
        let reply = WRef::new(HelloReply {
            message: message.as_bytes().into(),
        });

        Ok(reply)
    }
}

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    smol::block_on(async {
        let _server = mrpc::stub::Server::bind("0.0.0.0:5000")?
            .add_service(GreeterServer::new(MyGreeter::default()))
            .serve()
            .await?;
        Ok(())
    })
}
