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
    use mrpc::stub::{ClientStub, NamedService};

    #[derive(Debug)]
    pub struct GreeterClient {
        stub: ClientStub,
    }

    impl GreeterClient {
        // NOTE(cjr): We implement two separate `update_protos` for client and server because they
        // may find the proto file from differnt path (even from two different machines).
        fn update_protos() -> Result<(), ::mrpc::Error> {
            let srcs = [include_str!(
                "../../phoenix_examples/proto/rpc_hello/rpc_hello.proto"
            )];
            ::mrpc::stub::update_protos(srcs.as_slice())
        }

        pub fn connect<A: std::net::ToSocketAddrs>(dst: A) -> Result<Self, ::mrpc::Error> {
            // Force loading/reloading protos at the backend
            Self::update_protos()?;

            let stub = ClientStub::connect(dst).unwrap();
            Ok(Self { stub })
        }

        pub fn say_hello(
            &self,
            req: impl mrpc::IntoWRef<HelloRequest>,
        ) -> impl std::future::Future<Output = Result<mrpc::RRef<HelloReply>, ::mrpc::Status>> + '_
        {
            let call_id = self.stub.initiate_call();
            // Fill this with the right func_id
            let func_id = 3687134534u32;

            self.stub
                .unary(Self::SERVICE_ID, func_id, call_id, req.into_wref())
        }
    }

    impl NamedService for GreeterClient {
        const SERVICE_ID: u32 = 0;
        const NAME: &'static str = "rpc_hello.Greeter";
    }
}

pub mod greeter_server {
    use super::*;
    use mrpc::stub::{NamedService, Service};

    #[mrpc::async_trait]
    pub trait Greeter: Send + Sync + 'static {
        async fn say_hello(
            &self,
            request: mrpc::RRef<HelloRequest>,
        ) -> Result<mrpc::WRef<HelloReply>, mrpc::Status>;
    }

    /// Translate erased message to concrete type, and call the inner callback function.
    /// Translate the reply type to erased message again and put to write shared heap.
    #[derive(Debug)]
    pub struct GreeterServer<T: Greeter> {
        inner: T,
    }

    impl<T: Greeter> GreeterServer<T> {
        fn update_protos() -> Result<(), ::mrpc::Error> {
            let srcs = [include_str!(
                "../../phoenix_examples/proto/rpc_hello/rpc_hello.proto"
            )];
            ::mrpc::stub::update_protos(srcs.as_slice())
        }

        pub fn new(inner: T) -> Self {
            // TODO: handle error here
            Self::update_protos().unwrap();
            Self { inner }
        }
    }

    impl<T: Greeter> NamedService for GreeterServer<T> {
        const SERVICE_ID: u32 = 0;
        const NAME: &'static str = "rpc_hello.Greeter";
    }

    #[mrpc::async_trait]
    impl<T: Greeter> Service for GreeterServer<T> {
        async fn call(
            &self,
            req_opaque: mrpc::MessageErased,
            read_heap: std::sync::Arc<mrpc::ReadHeap>,
        ) -> (mrpc::WRefOpaque, mrpc::MessageErased) {
            let func_id = req_opaque.meta.func_id;
            match func_id {
                // TODO(cjr): fill this with the right func_id
                3687134534u32 => {
                    let req = ::mrpc::RRef::new(&req_opaque, read_heap);
                    let res = self.inner.say_hello(req).await;
                    match res {
                        Ok(reply) => ::mrpc::stub::service_post_handler(reply, &req_opaque),
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
