const SERVER_ADDR: &str = "192.168.211.194";
const SERVER_PORT: u16 = 5000;
use koala_examples::rpc_hello::mrpc;

// TODO(cjr): Put the code into rpc_hello.rs // generated code, used by both client.rs and
// server.rs

// Manually write all generated code
#[derive(Debug)]
struct HelloRequest {
    name: mrpc::Vec<u8>, // change to mrpc::alloc::Vec<u8>, -> String
}

#[derive(Debug)]
struct HelloReply {
    name: mrpc::Vec<u8>, // change to mrpc::alloc::Vec<u8>, -> String
}

#[derive(Debug)]
struct GreeterClient {
    inner: mrpc::Rpc,
}

impl GreeterClient {
    fn connect<A: ToSocketAddrs>(dst: A) -> Result<Self, mrpc::Error> {
        // use the cmid builder to create a CmId.
        // no you shouldn't rely on cmid here anymore. you should have your own rpc endpoint
        // cmid communicates directly to the transport engine. you need to pass your raw rpc
        // request/response to/from the rpc engine rather than the transport engine.
    }

    fn say_hello(
        &mut self,
        request: impl HelloRequest,
    ) -> impl Future<Output = Result<Response<HelloReply>, mrpc::Status>> {
    }
}


fn main() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
