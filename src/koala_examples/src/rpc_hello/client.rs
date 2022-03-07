use std::net::ToSocketAddrs;

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use libkoala::mrpc::stub::{
    ClientStub,
    Marshal, RpcMessage, RpcMsgType, SgList, ShmBuf, SwitchAddressSpace, Unmarshal,
};
use crate::rpc_hello::mrpc;

const SERVER_ADDR: &str = "192.168.211.194";
const SERVER_PORT: u16 = 5000;

// TODO(cjr): Put the code into rpc_hello.rs // generated code, used by both client.rs and
// server.rs

struct ReqFuture<'a> {
    stub: &'a ClientStub,
}

impl<'a> Future for ReqFuture<'a> {
    type Output = Result<HelloReply, mrpc::Status>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        todo!();
        Poll::Pending
    }
}

// Manually write all generated code
#[derive(Debug)]
struct HelloRequest {
    name: mrpc::Vec<u8>, // change to mrpc::alloc::Vec<u8>, -> String
}

#[derive(Debug)]
pub struct MessageMeta {
    conn_id: u32,
    func_id: u32,
    call_id: u64,
    len: u64,
    msg_type: RpcMsgType,
}

impl Unmarshal for MessageMeta {
    type Error = ();
    fn unmarshal(mut sg_list: SgList) -> Result<Self, Self::Error> {
        if sg_list.0.len() != 1 {
            return Err(());
        }
        sg_list.0[0]
    }
}

#[derive(Debug)]
pub struct MessageTemplate<T> {
    meta: MessageMeta,
    val: Unique<T>,
}

unsafe impl<T: SwitchAddressSpace> SwitchAddressSpace for MessageTemplate<T> {
    fn switch_address_space(&mut self) {
        unsafe { self.val.as_mut().switch_address_space() };
        todo!();
    }
}

impl<T: Unmarshal> Unmarshal for MessageTemplate<T> {
    type Error = ();
    fn unmarshal(mut sg_list: SgList) -> Result<Self, Self::Error> {
        if sg_list.0.len() <= 1 {
            return Err(());
        }
        let header_sgl = sg_list.0.remove(0);
        let meta = MessageMeta::unmarshal(header_sgl)?;
        let val = T::unmarshal(sg_list).or(Err(()))?;
        Ok(Self {
            meta,
            val,
        })
    }
}

impl<T: Send + Marshal + Unmarshal> RpcMessage for MessageTemplate<T> {
    #[inline]
    fn conn_id(&self) -> u32 { self.meta.conn_id }
    #[inline]
    fn func_id(&self) -> u32 { self.meta.func_id }
    #[inline]
    fn call_id(&self) -> u64 { self.meta.call_id }
    #[inline]
    fn len(&self) -> u64 { self.meta.len }
    #[inline]
    fn is_request(&self) -> bool { self.meta.msg_type == RpcMsgType::Request }
    fn marshal(&self) -> SgList {
        self.marshal().unwrap()
    }
}

#[derive(Debug)]
struct HelloReply {
    name: mrpc::Vec<u8>, // change to mrpc::alloc::Vec<u8>, -> String
}

#[derive(Debug)]
struct GreeterClient {
    stub: ClientStub,
}

impl GreeterClient {
    fn connect<A: ToSocketAddrs>(dst: A) -> Result<Self, mrpc::Error> {
        // use the cmid builder to create a CmId.
        // no you shouldn't rely on cmid here anymore. you should have your own rpc endpoint
        // cmid communicates directly to the transport engine. you need to pass your raw rpc
        // request/response to/from the rpc engine rather than the transport engine.
        // let stub = libkoala::mrpc::cm::MrpcStub::set_transport(libkoala::mrpc::cm::TransportType::Rdma)?;
        let stub = ClientStub::connect(dst).unwrap();
        Ok(Self { stub })
    }

    fn say_hello(
        &mut self,
        request: HelloRequest,
    ) -> impl Future<Output = Result<HelloReply, mrpc::Status>> + '_ {
        ReqFuture { stub: &self.stub }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    Ok(())
}
