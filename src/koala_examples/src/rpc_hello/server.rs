#![feature(allocator_api)]
use libkoala::mrpc::alloc::Vec;
use libkoala::mrpc::codegen::{Greeter, GreeterServer, HelloReply, HelloRequest};

#[derive(Debug)]
struct MyGreeter;

impl Greeter for MyGreeter {
    fn say_hello(
        &self,
        request: libkoala::mrpc::alloc::Box<HelloRequest>,
    ) -> Result<libkoala::mrpc::alloc::Box<HelloReply>, libkoala::mrpc::Status> {
        // eprintln!("resp: {:?}", request);
        let mut name = Vec::new();
        // name.extend(request.name);
        // NOTE(wyj): With our wrapped Box, we cannot perform DerefMove
        // see: https://manishearth.github.io/blog/2017/01/10/rust-tidbits-box-is-special/
        // we rely instead on an unbox method
        name.extend(request.unbox().name);
        let reply = libkoala::mrpc::alloc::Box::new(HelloReply { name });
        // eprintln!("reply: {:?}", reply);
        Ok(reply)
    }
}

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    let _server = libkoala::mrpc::stub::Server::bind("0.0.0.0:5000")?
        .add_service(GreeterServer::new(MyGreeter))
        .serve()?;
    Ok(())
}
