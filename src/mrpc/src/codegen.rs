// mimic the generated code of tonic-helloworld
// Manually writing all the generated code.

/// # Safety
//
/// The zero-copy inter-process communication thing is beyond what the compiler
/// can check. The programmer must ensure that everything is fine.

///  The request message containing the user's name.
#[derive(PartialEq, ::prost::Message)]
pub struct HelloRequest {
    #[prost(bytes = "vec", tag = "1")]
    pub name: ::mrpc::alloc::Vec<u8>,
}

///  The response message containing the greetings
#[derive(PartialEq, ::prost::Message)]
pub struct HelloReply {
    #[prost(bytes = "vec", tag = "1")]
    pub message: ::mrpc::alloc::Vec<u8>, // change to mrpc::alloc::Vec<u8>, -> String
}

pub mod greeter_client {
    use super::*;
    use mrpc::stub::{ClientStub, NamedService, RpcMessage};

    #[derive(Debug)]
    pub struct GreeterClient {
        stub: ClientStub,
        call_counter: std::cell::Cell<u32>,
    }

    impl GreeterClient {
        pub fn connect<A: std::net::ToSocketAddrs>(dst: A) -> Result<Self, ::mrpc::Error> {
            // use the cmid builder to create a CmId.
            // no you shouldn't rely on cmid here anymore. you should have your own rpc endpoint
            // cmid communicates directly to the transport engine. you need to pass your raw rpc
            // request/response to/from the rpc engine rather than the transport engine.
            // let stub = libkoala::mrpc::cm::MrpcStub::set_transport(libkoala::mrpc::cm::TransportType::Rdma)?;
            let stub = ClientStub::connect(dst).unwrap();
            Ok(Self {
                stub,
                call_counter: std::cell::Cell::new(0),
            })
        }

        pub fn say_hello(
            &self,
            msg: &RpcMessage<HelloRequest>,
        ) -> impl std::future::Future<
            Output = Result<::mrpc::shmview::ShmView<HelloReply>, ::mrpc::Status>,
        > + '_ {
            let call_id = self.call_counter.get();
            self.call_counter.set(call_id + 1);
            // TODO(cjr): fill this with the right func_id
            let func_id = 3687134534u32;

            self.stub.unary(Self::SERVICE_ID, func_id, call_id, msg)
        }
    }

    impl NamedService for GreeterClient {
        const SERVICE_ID: u32 = 0;
        const NAME: &'static str = "rpc_hello.Greeter";
    }
}

pub mod greeter_server {
    use super::*;
    use mrpc::stub::{NamedService, RpcMessage, Service};

    // #[async_trait]
    pub trait Greeter: Send + Sync + 'static {
        fn say_hello(
            &mut self,
            request: ::mrpc::shmview::ShmView<HelloRequest>,
        ) -> Result<&mut RpcMessage<HelloReply>, mrpc::Status>;
    }

    /// Translate erased message to concrete type, and call the inner callback function.
    /// Translate the reply type to erased message again and put to write shared heap.
    #[derive(Debug)]
    pub struct GreeterServer<T: Greeter> {
        inner: T,
    }

    impl<T: Greeter> GreeterServer<T> {
        pub fn new(inner: T) -> Self {
            Self { inner }
        }
    }

    impl<T: Greeter> NamedService for GreeterServer<T> {
        const SERVICE_ID: u32 = 0;
        const NAME: &'static str = "rpc_hello.Greeter";
    }

    impl<T: Greeter> Service for GreeterServer<T> {
        fn call(
            &mut self,
            req: mrpc::MessageErased,
            reclaim_buffer: &mrpc::stub::ReclaimBuffer,
        ) -> (::mrpc::MessageErased, u64) {
            let conn_id = req.meta.conn_id;
            let call_id = req.meta.call_id;
            let func_id = req.meta.func_id;
            match func_id {
                // TODO(cjr): fill this with the right func_id
                3687134534u32 => {
                    let req_view = ::mrpc::stub::service_pre_handler(&req, reclaim_buffer);
                    match self.inner.say_hello(req_view) {
                        Ok(reply) => ::mrpc::stub::service_post_handler(
                            reply,
                            conn_id,
                            Self::SERVICE_ID,
                            func_id,
                            call_id,
                        ),
                        Err(_status) => {
                            todo!();
                        }
                    }
                }
                _ => {
                    todo!("error handling for unknown func_id: {}", func_id);
                }
            }
        }
    }
}
