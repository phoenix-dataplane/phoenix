// mimic the generated code of rust stub
// Manually writing all the generated code.

#![no_main]

use cxx::CxxString;
use mrpc::{RRef, WRef};
use std::{net::SocketAddr, pin::Pin};

use incrementer_server::{Incrementer, IncrementerServer};

include!("typescodegen.rs");

#[cxx::bridge]
mod incrementer_ffi {
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

        unsafe fn run<'a>(
            addr: &CxxString,
            service: Pin<&'static mut CPPIncrementer<'a>>,
        ) -> Result<()>;
    }

    unsafe extern "C++" {
        type CPPIncrementer<'a>;

        include!("ffi/include/increment.h");
        fn incrementServer<'a>(
            self: Pin<&mut CPPIncrementer<'a>>,
            req: &ValueRequest,
        ) -> &ValueReply;
    }
}

// SERVER CODE

unsafe impl<'a> Send for incrementer_ffi::CPPIncrementer<'a> {}
unsafe impl<'a> Sync for incrementer_ffi::CPPIncrementer<'a> {}

use incrementer_ffi::CPPIncrementer;
fn run(
    addr: &CxxString,
    service: Pin<&'static mut CPPIncrementer>,
) -> Result<(), Box<dyn std::error::Error>> {
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
            .add_service(IncrementerServer::new(MyIncrementer { service: service }))
            .serve()
            .await?;
        Ok(())
    })
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
            let srcs = [include_str!(
                "../../../src/phoenix_examples/proto/rpc_int/rpc_int.proto"
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

struct MyIncrementer {
    pub service: Pin<&'static mut CPPIncrementer<'static>>,
}

impl Default for MyIncrementer {
    fn default() -> Self {
        todo!()
    }
}

#[mrpc::async_trait]
impl Incrementer for MyIncrementer {
    async fn increment(
        &self,
        request: RRef<ValueRequest>,
    ) -> Result<WRef<ValueReply>, mrpc::Status> {
        // eprintln!("request: {:?}", request);

        let field_addr = &self.service as *const _ as *const usize;
        let incrmenter_addr = unsafe { *field_addr };
        let incrementer_mut: &mut CPPIncrementer =
            unsafe { &mut *(incrmenter_addr as *mut CPPIncrementer) };

        // unsafe pointer
        let reply =
            unsafe { Pin::new_unchecked(incrementer_mut).incrementServer(request.as_ref()) };

        Ok(WRef::new(ValueReply { val: reply.val }))
    }
}
