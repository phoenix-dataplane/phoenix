use std::net::ToSocketAddrs;

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::mrpc;
use crate::mrpc::stub::{ClientStub, MessageTemplate};

// mimic the generated code of tonic-helloworld

/// # Safety
///
/// The zero-copy inter-process communication thing is beyond what the compiler
/// can check. The programmer must ensure that everything is fine.
pub unsafe trait SwitchAddressSpace {
    fn switch_address_space(&mut self);
}

// Manually write all generated code
#[derive(Debug)]
pub struct HelloRequest {
    pub name: mrpc::alloc::Vec<u8>, // change to mrpc::alloc::Vec<u8>, -> String
}

unsafe impl SwitchAddressSpace for HelloRequest {
    fn switch_address_space(&mut self) {
        self.name.switch_address_space();
    }
}

#[derive(Debug)]
pub struct HelloReply {
    pub name: mrpc::alloc::Vec<u8>, // change to mrpc::alloc::Vec<u8>, -> String
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
        request: HelloRequest,
    ) -> impl Future<Output = Result<HelloReply, mrpc::Status>> + '_ {
        let msg = MessageTemplate::new(request);
        self.stub.post(msg).unwrap();
        ReqFuture { stub: &self.stub }
    }
}
