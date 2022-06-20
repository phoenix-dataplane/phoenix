pub mod rpc_hello {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_hello");
}

use rpc_hello::greeter_server::{Greeter, GreeterServer};
use rpc_hello::{HelloReply, HelloRequest};

use mrpc::alloc::Vec;
use mrpc::shmview::ShmView;
use mrpc::stub::RpcMessage;

#[derive(Debug)]
struct MyGreeter {
    reply: RpcMessage<HelloReply>,
}

impl Greeter for MyGreeter {
    fn say_hello(
        &self,
        _request: ShmView<HelloRequest>,
    ) -> Result<&RpcMessage<HelloReply>, mrpc::Status> {
        eprintln!("reply: {:?}", self.reply);

        Ok(&self.reply)
    }
}

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    let _server = mrpc::stub::Server::bind("0.0.0.0:5000")?
        .add_service(GreeterServer::new(MyGreeter {
            reply: RpcMessage::new(HelloReply {
                message: Vec::new(),
            }),
        }))
        .serve()?;
    Ok(())
}
