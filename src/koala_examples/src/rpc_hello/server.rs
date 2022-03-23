#![feature(allocator_api)]
use libkoala::mrpc::alloc::Vec;
use libkoala::mrpc::codegen::{Greeter, GreeterServer, HelloReply, HelloRequest};
use libkoala::mrpc::shared_heap::SharedHeapAllocator;

#[derive(Debug)]
struct MyGreeter;

impl Greeter for MyGreeter {
    fn say_hello(
        &self,
        request: libkoala::mrpc::alloc::Box<HelloRequest>,
    ) -> Result<libkoala::mrpc::alloc::Box<HelloReply>, libkoala::mrpc::Status> {
        eprintln!("resp: {:?}", request);
        let mut name = Vec::new_in(SharedHeapAllocator);
        name.extend(request.name);
        let reply = Box::new_in(HelloReply { name }, SharedHeapAllocator);
        eprintln!("reply: {:?}", reply);
        Ok(reply)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _server = libkoala::mrpc::stub::Server::bind("0.0.0.0:5000")?
        .add_service(GreeterServer::new(MyGreeter))
        .serve()?;
    Ok(())
}
