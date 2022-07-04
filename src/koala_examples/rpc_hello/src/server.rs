pub mod rpc_hello {
    // The string specified here must match the proto package name
    // mrpc::include_proto!("rpc_hello");
    include!("../../../mrpc/src/codegen.rs");
}

use rpc_hello::greeter_server::{Greeter, GreeterServer};
use rpc_hello::{HelloReply, HelloRequest};

use mrpc::shmview::ShmView;
use mrpc::WRef;

#[derive(Debug, Default)]
struct MyGreeter {
    // reply: WRef<HelloReply>,
}

impl Greeter for MyGreeter {
    fn say_hello(&self, request: ShmView<HelloRequest>) -> Result<WRef<HelloReply>, mrpc::Status> {
        eprintln!("request: {:?}", request);

        let message = format!("Hello {}", String::from_utf8_lossy(&request.name));
        let reply = WRef::new(HelloReply {
            message: message.as_bytes().into(),
        });

        // Ok(&self.reply)
        Ok(reply)
    }
}

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    let _server = mrpc::stub::Server::bind("0.0.0.0:5000")?
        .add_service(GreeterServer::new(MyGreeter::default()))
        .serve()?;
    Ok(())
}
