pub mod rpc_int {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_int");
    // include!("../../../mrpc/src/codegen.rs");
}

use rpc_int::greeter_server::{Greeter, GreeterServer};
use rpc_int::{HelloReply, HelloRequest};

use mrpc::{RRef, WRef};

#[derive(Debug, Default)]
struct MyGreeter;

#[mrpc::async_trait]
impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: RRef<HelloRequest>,
    ) -> Result<WRef<HelloReply>, mrpc::Status> {
        eprintln!("request: {:?}", request);

        let reply = WRef::new(HelloReply {
            val: request.val + 1,
        });

        Ok(reply)
    }
}

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    smol::block_on(async {
        let mut server = mrpc::stub::LocalServer::bind("0.0.0.0:5000")?;
        server
            .add_service(GreeterServer::new(MyGreeter::default()))
            .serve()
            .await?;
        Ok(())
    })
}
