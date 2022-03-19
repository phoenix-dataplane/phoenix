use std::net::ToSocketAddrs;
use std::pin::Pin;

use std::future::Future;
use std::task::{Context, Poll};

// use crate::mrpc::shmptr::ShmPtr;

use crate::mrpc;
use crate::mrpc::shared_heap::SharedHeapAllocator;
use crate::mrpc::stub::{
    self, ClientStub, MessageTemplate, MessageTemplateErased, NamedService, Service,
};

// mimic the generated code of tonic-helloworld

/// # Safety
///
/// The zero-copy inter-process communication thing is beyond what the compiler
/// can check. The programmer must ensure that everything is fine.
pub unsafe trait SwitchAddressSpace {
    // An unsafe trait is unsafe to implement but safe to use.
    // The user of this trait does not need to satisfy any special condition.
    fn switch_address_space(&mut self);
}

// #[derive(Debug)]
// struct HelloRequestInner {
//     name: mrpc::alloc::Vec<u8>,
// }

// Manually write all generated code
#[derive(Debug)]
pub struct HelloRequest {
    pub name: mrpc::alloc::Vec<u8>,
    // inner: Pin<Unique<HelloRequestInner>>,
    // ptr: ShmPtr<HelloRequestInner, SharedHeapAllocator>,
    // ptr: Box<HelloRequestInner, SharedHeapAllocator>,
}

// impl HelloRequest {
//     #[inline]
//     pub fn name(&self) -> &mrpc::alloc::Vec<u8> {
//         &self.ptr.name
//     }
// }

unsafe impl SwitchAddressSpace for HelloRequest {
    fn switch_address_space(&mut self) {
        self.name.switch_address_space();
    }
}

#[derive(Debug)]
pub struct HelloReply {
    pub name: mrpc::alloc::Vec<u8>, // change to mrpc::alloc::Vec<u8>, -> String
}

unsafe impl SwitchAddressSpace for HelloReply {
    fn switch_address_space(&mut self) {
        self.name.switch_address_space();
    }
}

pub struct ReqFuture<'a> {
    stub: &'a ClientStub,
}

impl<'a> Future for ReqFuture<'a> {
    type Output = Result<HelloReply, mrpc::Status>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        todo!();
        Poll::Pending
    }
}

#[derive(Debug)]
pub struct GreeterClient {
    stub: ClientStub,
}

impl GreeterClient {
    pub fn connect<A: ToSocketAddrs>(dst: A) -> Result<Self, mrpc::Error> {
        // use the cmid builder to create a CmId.
        // no you shouldn't rely on cmid here anymore. you should have your own rpc endpoint
        // cmid communicates directly to the transport engine. you need to pass your raw rpc
        // request/response to/from the rpc engine rather than the transport engine.
        // let stub = libkoala::mrpc::cm::MrpcStub::set_transport(libkoala::mrpc::cm::TransportType::Rdma)?;
        let stub = ClientStub::connect(dst).unwrap();
        Ok(Self { stub })
    }

    pub fn say_hello(
        &mut self,
        request: mrpc::alloc::Box<HelloRequest>,
    ) -> impl Future<Output = Result<HelloReply, mrpc::Status>> + '_ {
        let msg = MessageTemplate::new(request, self.stub.get_handle());
        stub::post_call(msg).unwrap();
        ReqFuture { stub: &self.stub }
    }
}

// #[async_trait]
pub trait Greeter: Send + Sync + 'static {
    fn say_hello(
        &self,
        request: mrpc::alloc::Box<HelloRequest>,
    ) -> Result<mrpc::alloc::Box<HelloReply>, mrpc::Status>;
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
    const FUNC_ID: u32 = 0;
}

impl<T: Greeter> Service for GreeterServer<T> {
    fn call(
        &mut self,
        req: interface::rpc::MessageTemplateErased,
    ) -> interface::rpc::MessageTemplateErased {
        assert_eq!(Self::FUNC_ID, req.meta.func_id);
        let conn_id = req.meta.conn_id;
        let raw = req.shmptr as *mut MessageTemplate<HelloRequest>;
        let msg = unsafe { mrpc::alloc::Box::from_raw_in(raw, SharedHeapAllocator) };
        let req = unsafe { mrpc::alloc::Box::from_raw_in(msg.val.as_ptr(), SharedHeapAllocator) };
        std::mem::forget(msg);
        match self.inner.say_hello(req) {
            Ok(reply) => {
                let mut msg = MessageTemplate::new_reply(reply, conn_id);
                msg.switch_address_space();
                let erased = MessageTemplateErased {
                    meta: msg.meta,
                    shmptr: mrpc::alloc::Box::into_raw(msg) as u64,
                };
                erased
            }
            Err(_status) => {
                todo!();
            }
        }
    }
}
