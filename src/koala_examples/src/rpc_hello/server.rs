#![feature(allocator_api)]
use libkoala::mrpc::alloc::Vec;
use libkoala::mrpc::codegen::{Greeter, GreeterServer, HelloReply, HelloRequest};
use libkoala::mrpc::stub::{MessageTemplate, NamedService};

#[derive(Debug)]
struct MyGreeter {
    reply: libkoala::mrpc::alloc::Box::<MessageTemplate<HelloReply>>
}

impl Greeter for MyGreeter {
    fn say_hello(
        &mut self,
        _request: libkoala::mrpc::alloc::Box<HelloRequest>,
    ) -> Result<&mut libkoala::mrpc::alloc::Box<MessageTemplate<HelloReply>>, libkoala::mrpc::Status> {
        // eprintln!("reply: {:?}", reply);
        Ok(&mut self.reply)
    }
}

fn main() -> Result<(), std::boxed::Box<dyn std::error::Error>> {
    let mut name = Vec::with_capacity(1000000);
    name.resize(1000000, 42);

    let reply = libkoala::mrpc::alloc::Box::new(HelloReply { name });

    let msg = MessageTemplate::new_reply(
        reply,
        interface::Handle(0), 
        GreeterServer::<MyGreeter>::FUNC_ID, 
        0
    );

    let _server = libkoala::mrpc::stub::Server::bind("0.0.0.0:5000")?
        .add_service(GreeterServer::new(MyGreeter { reply: msg }))
        .serve()?;
    Ok(())
}
