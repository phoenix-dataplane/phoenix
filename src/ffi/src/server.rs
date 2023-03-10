#![no_main]
#![allow(unused_variables)]
#![allow(unused_imports)]
#![allow(dead_code)]

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
use mrpc::alloc::{Vec};
////////////////////////////// *** server code *** ///////////////////////////////////

use incrementer_server::{IncrementerServer, Incrementer};

// Types
#[derive(Debug, Default, Clone)]
pub struct ValueRequest {
    pub val: u64,
    pub key: ::mrpc::alloc::Vec<u8>,
}

fn new_value_request() -> Box<ValueRequest> {
    Box::new( ValueRequest {
        val: 0,
        key: Vec::new()
    })
}

impl ValueRequest {
    fn val(&self) -> u64 {
        self.val
    }

    fn set_val(&mut self, val: u64) {
        self.val = val;
    }

    fn key(&self, index: usize) -> u8 {
        self.key[index]
    }

    fn key_size(&self) -> usize {
        self.key.len()
    }

    fn set_key(&mut self, index: usize, value: u8) {
        self.key[index] = value;
    }

    fn add_foo(&mut self, value: u8) {
        self.key.push(value);
    }
}

#[derive(Debug, Default, Copy, Clone)]
pub struct ValueReply {
    pub val: u64,
}

fn new_value_reply() -> Box<ValueReply> {
    Box::new(ValueReply { val: 0 })
}

impl ValueReply {
    fn val(&self) -> u64 {
        self.val
    }

    fn set_val(&mut self, val: u64) {
        self.val = val
    }
}
// Types end

// unsafe impl ExternType for ValueRequest {
//     type Id = type_id!("ValueRequest");
//     type Kind = cxx::kind::Trivial;
// }

// unsafe impl ExternType for rpc_int::ValueReply {
//     type Id = type_id!("ValueReply");
//     type Kind = cxx::kind::Trivial;
// }


unsafe impl<'a> Send for rpc_handler_lib::CPPIncrementer<'a>{}
unsafe impl<'a> Sync for rpc_handler_lib::CPPIncrementer<'a>{}

#[cxx::bridge]
mod rpc_handler_lib {
    extern "Rust" {
        type ValueRequest;
        type ValueReply;

        fn new_value_request() -> Box<ValueRequest>;
        fn val(self: &ValueRequest) -> u64;
        fn set_val(self: &mut ValueRequest, val: u64);
        fn key(self: &ValueRequest, index: usize) -> u8;
        fn key_size(self: &ValueRequest) -> usize;
        fn set_key(self: &mut ValueRequest, index: usize, value: u8);
        fn add_foo(self: &mut ValueRequest, value: u8);
        
        fn new_value_reply() -> Box<ValueReply>;
        fn val(self: &ValueReply) -> u64;
        fn set_val(self: &mut ValueReply, val: u64);
        
        unsafe fn run<'a>(addr: &CxxString, service: Pin<&'static mut CPPIncrementer<'a>>) -> Result<()>;
    }

    unsafe extern "C++" {
        type CPPIncrementer<'a>;

        include!("ffi/include/increment.h");
        fn incrementServer<'a>(self: Pin<&mut CPPIncrementer<'a>>, req: &ValueRequest) -> &ValueReply;
    }
}

#[cxx::bridge]
mod server_entry {
    
}

use rpc_handler_lib::CPPIncrementer;
fn run(addr: &CxxString, service: Pin<&'static mut CPPIncrementer>) -> Result<(), Box<dyn Error>> {
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

// pub mod rpc_int {
//     #[derive(Debug, Default)]
//     pub struct ValueReply {
//         pub val: i32,
//     }

//     #[derive(Debug, Default)]
//     pub struct ValueRequest {
//         pub val: i32,
//     }
// }

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
            let srcs = [include_str!(
                "../../phoenix_examples/proto/rpc_int/rpc_int.proto"
            )];
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

// pub mod proto {
//     pub const PROTO_SRCS: &[&str] = &[
//         "syntax = \"proto3\";\n\npackage rpc_int;\n\nservice Incrementer {\n  // increments an int  \n  rpc Increment(ValueRequest) returns (ValueReply) {}\n}\n\nmessage ValueRequest {\n  uint64 val = 1;\n}\n\nmessage ValueReply {\n  uint64 val = 1;\n}\n",
//     ];
// }
use std::pin::Pin;

struct MyIncrementer {
    pub service: Pin<&'static mut CPPIncrementer<'static>>,
}

impl Default for MyIncrementer {
    fn default() -> Self { todo!() }
}


#[mrpc::async_trait]
impl Incrementer for MyIncrementer{
    async fn increment(
        &self,
        request: RRef<ValueRequest>,
    ) -> Result<WRef<ValueReply>, mrpc::Status> {
        // eprintln!("request: {:?}", request);


        let field_addr = &self.service as *const _ as *const usize;
        let incrmenter_addr = unsafe { *field_addr };
        let incrementer_mut: &mut CPPIncrementer = unsafe { &mut *(incrmenter_addr as *mut CPPIncrementer) };

        // unsafe pointer 
        let reply = unsafe {Pin::new_unchecked(incrementer_mut).incrementServer(request.as_ref())};

        Ok(WRef::new(ValueReply{
           val: reply.val,
        }))
    }
}
