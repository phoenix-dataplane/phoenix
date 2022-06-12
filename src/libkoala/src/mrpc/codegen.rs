use std::net::ToSocketAddrs;
use std::pin::Pin;

use std::future::Future;
use std::task::{Context, Poll};

use interface::rpc::{MessageMeta, RpcMsgType};

// use crate::mrpc::shmptr::ShmPtr;
use crate::mrpc;
use crate::mrpc::alloc::{ShmView, ShmRecvContext};
use crate::mrpc::stub::{
    ClientStub, MessageErased, NamedService, RpcMessage, Service,
};
use crate::salloc::owner::{AppOwned, BackendOwned};

use super::stub::{check_completion_queue, RECV_REPLY_CACHE};

// mimic the generated code of tonic-helloworld

// # Safety
//
// The zero-copy inter-process communication thing is beyond what the compiler
// can check. The programmer must ensure that everything is fine.

// Manually write all generated code

pub type HelloRequest = inner::HelloRequest;
pub type HelloReply = inner::HelloReply;

mod inner {
    use crate::mrpc;
    use crate::mrpc::alloc::CloneFromBackendOwned;
    use crate::salloc::owner::{AllocOwner, AppOwned, BackendOwned};

    #[derive(Debug)]
    pub struct HelloRequest<O: AllocOwner = AppOwned> {
        pub name: mrpc::alloc::Vec<u8, O>,
    }

    impl CloneFromBackendOwned for HelloRequest<AppOwned> {
        type BackendOwned = HelloRequest<BackendOwned>;

        fn clone_from_backend_owned(backend_owned: &Self::BackendOwned) -> Self {
            HelloRequest {
                name: mrpc::alloc::Vec::clone_from_backend_owned(&backend_owned.name),
            }
        }
    }

    #[derive(Debug)]
    pub struct HelloReply<O: AllocOwner = AppOwned> {
        pub name: mrpc::alloc::Vec<u8, O>, // change to mrpc::alloc::Vec<u8>, -> String
    }

    impl CloneFromBackendOwned for HelloReply<AppOwned> {
        type BackendOwned = HelloReply<BackendOwned>;

        fn clone_from_backend_owned(backend_owned: &Self::BackendOwned) -> Self {
            HelloReply {
                name: mrpc::alloc::Vec::clone_from_backend_owned(&backend_owned.name),
            }
        }
    }
}

pub struct ReqFuture<'a> {
    conn_id: interface::Handle,
    call_id: u32,
    ctx: ShmRecvContext<'a>,
}

impl<'a> Future for ReqFuture<'a> {
    type Output = Result<ShmView<'a, HelloReply>, mrpc::Status>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        use ipc::mrpc::dp::WRIdentifier;

        let this = self.get_mut();
        check_completion_queue();
        if let Some(erased) =
            RECV_REPLY_CACHE.with(|cache| cache.borrow_mut().remove(&WRIdentifier(this.conn_id, this.call_id)))
        {
            tracing::trace!(
                "ReqFuture receive reply from mRPC engine, call_id={}",
                erased.meta.call_id
            );
            let ptr_app = erased.shm_addr_app
                as *mut inner::HelloReply<BackendOwned>;
            let ptr_backend = ptr_app.with_addr(erased.shm_addr_backend);
            let msg = unsafe { mrpc::alloc::Box::from_backend_raw(ptr_app, ptr_backend) };
            let reply = ShmView::new_from_backend_owned(msg, this.ctx);
            Poll::Ready(Ok(reply))
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}


#[derive(Debug)]
pub struct GreeterClient {
    stub: ClientStub,
    call_counter: std::cell::Cell<u32>,
}

impl GreeterClient {
    pub fn connect<A: ToSocketAddrs>(dst: A) -> Result<Self, mrpc::Error> {
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
        msg: &mut RpcMessage<inner::HelloRequest<AppOwned>>,
    ) -> impl Future<Output = Result<ShmView<HelloReply>, mrpc::Status>> + '_ {
        let conn_id = self.stub.get_handle();
        let call_id = self.call_counter.get();
        self.call_counter.set(call_id + 1);

        // construct meta
        let meta = MessageMeta {
            conn_id,
            func_id: Self::FUNC_ID,
            call_id,
            len: 0,
            msg_type: RpcMsgType::Request,
        };

        // increase send count for RpcMessage
        msg.send_count += 1;
        self.stub.post_request(msg, meta).unwrap();
        
        let ctx = ShmRecvContext::new(self);

        ReqFuture { conn_id, call_id, ctx }
    }
}

impl NamedService for GreeterClient {
    const FUNC_ID: u32 = 0;
}

// #[async_trait]
pub trait Greeter: Send + Sync + 'static {
    fn say_hello(
        &mut self,
        request: ShmView<HelloRequest>,
    ) -> Result<&mut RpcMessage<inner::HelloReply>, mrpc::Status>;
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
        req: interface::rpc::MessageErased,
    ) -> (interface::rpc::MessageErased, u64) {
        assert_eq!(Self::FUNC_ID, req.meta.func_id);
        let conn_id = req.meta.conn_id;
        let call_id = req.meta.call_id;

        let ptr_app =
            req.shm_addr_app as *mut inner::HelloRequest<BackendOwned>;
        // TODO(wyj): refine the following line, this pointer may be invalid.
        // should we directly constrct a pointer using remote addr?
        // or just keep the addr u64?
        let ptr_backend = ptr_app.with_addr(req.shm_addr_backend);
        let msg = unsafe { mrpc::alloc::Box::from_backend_raw(ptr_app, ptr_backend) };

        // TODO(wyj): lifetime bound for ShmView
        // ShmView should be !Send and !Sync
        let ctx = ();
        let req = ShmView::new_from_backend_owned(msg, ShmRecvContext::new(&ctx));


        match self.inner.say_hello(req) {
            Ok(reply) => {
                // construct meta
                let meta = MessageMeta {
                    conn_id,
                    func_id: Self::FUNC_ID,
                    call_id,
                    len: 0,
                    msg_type: RpcMsgType::Response
                };

                // increase send count of RpcMessage
                reply.send_count += 1;

                let (ptr_app, ptr_backend) = mrpc::alloc::Box::to_raw_parts(&reply.inner);
                let erased = MessageErased {
                    meta,
                    shm_addr_app: ptr_app.addr().get(),
                    shm_addr_backend: ptr_backend.addr().get(),
                };
                (erased, reply.identifier)
            }
            Err(_status) => {
                todo!();
            }
        }
    }
}
