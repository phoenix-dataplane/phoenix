#![feature(prelude_import)]
#![no_main]
#[prelude_import]
use std::prelude::rust_2021::*;
#[macro_use]
extern crate std;
use std::{net::SocketAddr, os::fd::AsRawFd, error::Error};
use memfd::Memfd;
use std::io;
use cxx::{CxxString, ExternType, type_id};
use interface::{Handle, rpc::{MessageMeta, RpcMsgType}};
use ipc::mrpc::{
    cmd::{Command, CompletionKind},
    dp::{self, WorkRequest},
};
use ipc::salloc::cmd::Command as SallocCommand;
use ipc::salloc::cmd::CompletionKind as SallocCompletion;
use salloc::backend::SA_CTX;
use mrpc::{MRPC_CTX, MessageErased, RRef, WRef};
use ipc::mrpc::cmd::ReadHeapRegion;
use incrementer_server::{IncrementerServer, Incrementer};
unsafe impl ExternType for rpc_int::ValueRequest {
    type Id = (
        ::cxx::V,
        ::cxx::a,
        ::cxx::l,
        ::cxx::u,
        ::cxx::e,
        ::cxx::R,
        ::cxx::e,
        ::cxx::q,
        ::cxx::u,
        ::cxx::e,
        ::cxx::s,
        ::cxx::t,
    );
    type Kind = cxx::kind::Trivial;
}
unsafe impl ExternType for rpc_int::ValueReply {
    type Id = (
        ::cxx::V,
        ::cxx::a,
        ::cxx::l,
        ::cxx::u,
        ::cxx::e,
        ::cxx::R,
        ::cxx::e,
        ::cxx::p,
        ::cxx::l,
        ::cxx::y,
    );
    type Kind = cxx::kind::Trivial;
}
unsafe impl<'a> Send for rpc_handler_lib::CPPIncrementer<'a> {}
unsafe impl<'a> Sync for rpc_handler_lib::CPPIncrementer<'a> {}
#[deny(improper_ctypes, improper_ctypes_definitions)]
#[allow(clippy::unknown_clippy_lints)]
#[allow(non_camel_case_types, non_snake_case, clippy::upper_case_acronyms)]
mod server_entry {}
use rpc_handler_lib::CPPIncrementer;
fn run(
    addr: &CxxString,
    service: Pin<&'static mut CPPIncrementer>,
) -> Result<(), Box<dyn Error>> {
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
pub mod rpc_int {
    pub struct ValueReply {
        pub val: i32,
    }
    #[automatically_derived]
    impl ::core::fmt::Debug for ValueReply {
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            ::core::fmt::Formatter::debug_struct_field1_finish(
                f,
                "ValueReply",
                "val",
                &&self.val,
            )
        }
    }
    #[automatically_derived]
    impl ::core::default::Default for ValueReply {
        #[inline]
        fn default() -> ValueReply {
            ValueReply {
                val: ::core::default::Default::default(),
            }
        }
    }
    pub struct ValueRequest {
        pub val: i32,
    }
    #[automatically_derived]
    impl ::core::fmt::Debug for ValueRequest {
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            ::core::fmt::Formatter::debug_struct_field1_finish(
                f,
                "ValueRequest",
                "val",
                &&self.val,
            )
        }
    }
    #[automatically_derived]
    impl ::core::default::Default for ValueRequest {
        #[inline]
        fn default() -> ValueRequest {
            ValueRequest {
                val: ::core::default::Default::default(),
            }
        }
    }
}
pub mod incrementer_server {
    use ::mrpc::stub::{NamedService, Service};
    pub trait Incrementer: Send + Sync + 'static {
        /// increments an int
        #[must_use]
        #[allow(clippy::type_complexity, clippy::type_repetition_in_bounds)]
        fn increment<'life0, 'async_trait>(
            &'life0 self,
            request: ::mrpc::RRef<super::ValueRequest>,
        ) -> ::core::pin::Pin<
            Box<
                dyn ::core::future::Future<
                    Output = Result<::mrpc::WRef<super::ValueReply>, ::mrpc::Status>,
                > + ::core::marker::Send + 'async_trait,
            >,
        >
        where
            'life0: 'async_trait,
            Self: 'async_trait;
    }
    pub struct IncrementerServer<T: Incrementer> {
        inner: T,
    }
    #[automatically_derived]
    impl<T: ::core::fmt::Debug + Incrementer> ::core::fmt::Debug
    for IncrementerServer<T> {
        fn fmt(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
            ::core::fmt::Formatter::debug_struct_field1_finish(
                f,
                "IncrementerServer",
                "inner",
                &&self.inner,
            )
        }
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
    impl<T: Incrementer> Service for IncrementerServer<T> {
        #[allow(
            clippy::let_unit_value,
            clippy::no_effect_underscore_binding,
            clippy::shadow_same,
            clippy::type_complexity,
            clippy::type_repetition_in_bounds,
            clippy::used_underscore_binding
        )]
        fn call<'life0, 'async_trait>(
            &'life0 self,
            req_opaque: ::mrpc::MessageErased,
            read_heap: std::sync::Arc<::mrpc::ReadHeap>,
        ) -> ::core::pin::Pin<
            Box<
                dyn ::core::future::Future<
                    Output = (::mrpc::WRefOpaque, ::mrpc::MessageErased),
                > + ::core::marker::Send + 'async_trait,
            >,
        >
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async move {
                if let ::core::option::Option::Some(__ret)
                    = ::core::option::Option::None::<
                        (::mrpc::WRefOpaque, ::mrpc::MessageErased),
                    > {
                    return __ret;
                }
                let __self = self;
                let req_opaque = req_opaque;
                let read_heap = read_heap;
                let __ret: (::mrpc::WRefOpaque, ::mrpc::MessageErased) = {
                    let func_id = req_opaque.meta.func_id;
                    match func_id {
                        3784353755u32 => {
                            let req = ::mrpc::RRef::new(&req_opaque, read_heap);
                            let res = __self.inner.increment(req).await;
                            match res {
                                Ok(reply) => {
                                    ::mrpc::stub::service_post_handler(reply, &req_opaque)
                                }
                                Err(_status) => {
                                    ::core::panicking::panic("not yet implemented");
                                }
                            }
                        }
                        _ => {
                            ::core::panicking::panic_fmt(
                                ::core::fmt::Arguments::new_v1(
                                    &["not yet implemented: "],
                                    &[
                                        ::core::fmt::ArgumentV1::new_display(
                                            &::core::fmt::Arguments::new_v1(
                                                &["error handling for unknown func_id: "],
                                                &[::core::fmt::ArgumentV1::new_display(&func_id)],
                                            ),
                                        ),
                                    ],
                                ),
                            );
                        }
                    }
                };
                #[allow(unreachable_code)] __ret
            })
        }
    }
}
pub mod proto {
    pub const PROTO_SRCS: &[&str] = &[
        "syntax = \"proto3\";\n\npackage rpc_int;\n\nservice Incrementer {\n  // increments an int  \n  rpc Increment(ValueRequest) returns (ValueReply) {}\n}\n\nmessage ValueRequest {\n  uint64 val = 1;\n}\n\nmessage ValueReply {\n  uint64 val = 1;\n}\n",
    ];
}
use rpc_int::{ValueRequest, ValueReply};
use std::pin::Pin;
struct MyIncrementer<'a> {
    pub service: Pin<&'a mut CPPIncrementer<'a>>,
}
impl<'a> Default for MyIncrementer<'a> {
    fn default() -> Self {
        ::core::panicking::panic("not yet implemented")
    }
}
impl Incrementer for MyIncrementer<'static> {
    #[allow(
        clippy::let_unit_value,
        clippy::no_effect_underscore_binding,
        clippy::shadow_same,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds,
        clippy::used_underscore_binding
    )]
    fn increment<'life0, 'async_trait>(
        &'life0 self,
        request: RRef<ValueRequest>,
    ) -> ::core::pin::Pin<
        Box<
            dyn ::core::future::Future<
                Output = Result<WRef<ValueReply>, mrpc::Status>,
            > + ::core::marker::Send + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            if let ::core::option::Option::Some(__ret)
                = ::core::option::Option::None::<
                    Result<WRef<ValueReply>, mrpc::Status>,
                > {
                return __ret;
            }
            let __self = self;
            let request = request;
            let __ret: Result<WRef<ValueReply>, mrpc::Status> = {
                {
                    ::std::io::_eprint(
                        ::core::fmt::Arguments::new_v1(
                            &["request: ", "\n"],
                            &[::core::fmt::ArgumentV1::new_debug(&request)],
                        ),
                    );
                };
                let reply = __self
                    .service
                    .incrementServer(rpc_int::ValueRequest {
                        val: request.val,
                    });
                Ok(WRef::new(ValueReply { val: reply.val }))
            };
            #[allow(unreachable_code)] __ret
        })
    }
}
