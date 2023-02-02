#![no_main]

use std::{net::SocketAddr, os::fd::AsRawFd, error::Error};
use memfd::Memfd;
use std::io;
use cxx::{CxxString, ExternType, type_id};
use interface::{Handle, rpc::{MessageMeta, RpcMsgType}};
use ipc::mrpc::{cmd::{Command, CompletionKind}, dp::{self, WorkRequest}};
use ipc::salloc::cmd::Command as SallocCommand;
use ipc::salloc::cmd::CompletionKind as SallocCompletion;
use salloc::backend::SA_CTX;
use mrpc::{MRPC_CTX, MessageErased, RRef, WRef};
use ipc::mrpc::cmd::ReadHeapRegion;
////////////////////////////// *** server code *** ///////////////////////////////////

use incrementer_server::{IncrementerServer, Incrementer};

unsafe impl ExternType for rpc_int::ValueRequest {
    type Id = type_id!("ValueRequest");
    type Kind = cxx::kind::Trivial;
}

unsafe impl ExternType for rpc_int::ValueReply {
    type Id = type_id!("ValueReply");
    type Kind = cxx::kind::Trivial;
}

unsafe impl Send for rpc_handler_lib::CPPIncrementer{}
unsafe impl Sync for rpc_handler_lib::CPPIncrementer{}

#[cxx::bridge]
mod rpc_handler_lib {
    unsafe extern "C++" {
        type CPPIncrementer;

        include!("ffi/include/increment.h");
        type ValueRequest = crate::rpc_int::ValueRequest;
        type ValueReply = crate::rpc_int::ValueReply;
        fn incrementServer(self: &CPPIncrementer, req: ValueRequest) -> ValueReply;
    }

    extern "Rust" {
        fn run(addr: &CxxString, service: &CPPIncrementer) -> Result<()>;
    }
}

#[cxx::bridge]
mod server_entry {
    
}

use rpc_handler_lib::CPPIncrementer;
fn run(addr: &CxxString, service: &CPPIncrementer) -> Result<(), Box<dyn Error>> {
    let addr_as_str = match addr.to_str() {
        Ok(s) => s,
        Err(e) => return Err(Box::new(e)),
    };
    let server: SocketAddr = match addr_as_str.parse() {
        Ok(s) => s,
        Err(e) => return Err(Box::new(e)),
    };
    smol::block_on(async {
    let mut server = mrpc::stub::LocalServer::bind(server)?;
        server
            .add_service(IncrementerServer::new(MyIncrementer {
                service: service,
            }))
            .serve()
            .await?;
        Ok(())
    })
}

pub mod rpc_int {
    #[derive(Debug, Default)]
    pub struct ValueReply {
        pub val: i32,
    }

    #[derive(Debug, Default)]
    pub struct ValueRequest {
        pub val: i32,
    }
}

pub mod incrementer_server {
    use ::mrpc::stub::{NamedService, Service};
    #[mrpc::async_trait]
    pub trait Incrementer: Send + Sync + 'static {
        /// increments an int
        async fn increment(
            &self,
            request: ::mrpc::RRef<super::ValueRequest>,
        ) -> Result<::mrpc::WRef<super::ValueReply>, ::mrpc::Status>;
    }
    #[derive(Debug)]
    pub struct IncrementerServer<T: Incrementer> {
        inner: T,
    }
    impl<T: Incrementer> IncrementerServer<T> {
        fn update_protos() -> Result<(), ::mrpc::Error> {
            let srcs = [super::proto::PROTO_SRCS].concat();
            ::mrpc::stub::update_protos(srcs.as_slice())
        }
        pub fn new(inner: T) -> Self {
            Self::update_protos().unwrap();
            Self { inner }
        }
    }
    impl<T: Incrementer> NamedService for IncrementerServer<T> {
        const SERVICE_ID: u32 = 2056765301u32;
        const NAME: &'static str = "rpc_int.Incrementer";
    }
    #[mrpc::async_trait]
    impl<T: Incrementer> Service for IncrementerServer<T> {
        async fn call(
            &self,
            req_opaque: ::mrpc::MessageErased,
            read_heap: std::sync::Arc<::mrpc::ReadHeap>,
        ) -> (::mrpc::WRefOpaque, ::mrpc::MessageErased) {
            let func_id = req_opaque.meta.func_id;
            match func_id {
                3784353755u32 => {
                    let req = ::mrpc::RRef::new(&req_opaque, read_heap);
                    let res = self.inner.increment(req).await;
                    match res {
                        Ok(reply) => {
                            ::mrpc::stub::service_post_handler(reply, &req_opaque)
                        }
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

pub mod proto {
    pub const PROTO_SRCS: &[&str] = &[
        "syntax = \"proto3\";\n\npackage rpc_int;\n\nservice Incrementer {\n  // increments an int  \n  rpc Increment(ValueRequest) returns (ValueReply) {}\n}\n\nmessage ValueRequest {\n  uint64 val = 1;\n}\n\nmessage ValueReply {\n  uint64 val = 1;\n}\n",
    ];
}
use rpc_int::{ValueRequest, ValueReply};

struct MyIncrementer<'a> {
    pub service: &'a CPPIncrementer,
}

impl Default for MyIncrementer<'_> {
    fn default() -> Self { todo!() }
}

#[mrpc::async_trait]
impl Incrementer for MyIncrementer<'_> {
    async fn increment(
        &self,
        request: RRef<ValueRequest>,
    ) -> Result<WRef<ValueReply>, mrpc::Status> {
        eprintln!("request: {:?}", request);

        let reply = self.service.incrementServer(rpc_int::ValueRequest{val: request.val});

        Ok(WRef::new(ValueReply{
           val: reply.val,
        }))
    }
}
