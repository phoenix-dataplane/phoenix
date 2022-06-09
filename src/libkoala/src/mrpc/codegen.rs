use std::net::ToSocketAddrs;
use std::pin::Pin;

use std::future::Future;
use std::task::{Context, Poll};

use ipc::shmalloc::SwitchAddressSpace;

// use crate::mrpc::shmptr::ShmPtr;
use crate::mrpc;
use crate::mrpc::alloc::{ShmView, ShmRecvContext};
use crate::mrpc::stub::{
    ClientStub, MessageTemplate, MessageTemplateErased, NamedService, RpcMessage, Service,
};
use crate::salloc::owner::{AppOwned, BackendOwned};

use super::stub::ownership::{AppOwendReply, AppOwendRequest};
use super::stub::{check_completion_queue, RECV_CACHE};

// mimic the generated code of tonic-helloworld

// # Safety
//
// The zero-copy inter-process communication thing is beyond what the compiler
// can check. The programmer must ensure that everything is fine.

// Manually write all generated code

pub type HelloRequest = inner::HelloRequest;
pub type HelloReply = inner::HelloReply;

impl AppOwendRequest for HelloRequest {}
impl AppOwendReply for HelloReply {}

mod inner {
    use ipc::shmalloc::SwitchAddressSpace;

    use crate::mrpc;
    use crate::mrpc::alloc::CloneFromBackendOwned;
    use crate::salloc::owner::{AllocOwner, AppOwned, BackendOwned};

    #[derive(Debug)]
    pub struct HelloRequest<O: AllocOwner = AppOwned> {
        pub name: mrpc::alloc::Vec<u8, O>,
    }

    unsafe impl SwitchAddressSpace for HelloRequest<AppOwned> {
        fn switch_address_space(&mut self) {
            self.name.switch_address_space();
        }
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

    unsafe impl SwitchAddressSpace for HelloReply<AppOwned> {
        fn switch_address_space(&mut self) {
            self.name.switch_address_space();
        }
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
        let this = self.get_mut();
        check_completion_queue();
        if let Some(erased) =
            RECV_CACHE.with(|cache| cache.borrow_mut().remove(&(this.conn_id, this.call_id)))
        {
            tracing::trace!(
                "ReqFuture receive reply from mRPC engine, call_id={}",
                erased.meta.call_id
            );
            let ptr_local = erased.shm_addr
                as *mut MessageTemplate<inner::HelloReply<BackendOwned>, BackendOwned>;
            let ptr_remote = ptr_local.with_addr(erased.shm_addr_remote);
            let msg = unsafe { mrpc::alloc::Box::from_backend_raw(ptr_local, ptr_remote) };
            let reply = unsafe { mrpc::alloc::Box::from_backend_shmptr(msg.val) };
            let reply = ShmView::new_from_backend_owned(reply, this.ctx);
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
    call_counter: std::cell::RefCell<u32>,
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
            call_counter: std::cell::RefCell::new(0),
        })
    }

    pub fn say_hello(
        &self,
        msg: &mut RpcMessage<inner::HelloRequest<AppOwned>>,
    ) -> impl Future<Output = Result<ShmView<HelloReply>, mrpc::Status>> + '_ {
        let conn_id = self.stub.get_handle();
        let mut call_counter = self.call_counter.borrow_mut();
        let call_id = *call_counter;
        msg.inner.meta.conn_id = conn_id;
        msg.inner.meta.call_id = call_id;
        msg.inner.meta.func_id = Self::FUNC_ID;

        // increase send count for RpcMessage
        msg.send_count += 1;

        *call_counter += 1;

        self.stub.post_request(msg).unwrap();
        
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
        req: interface::rpc::MessageTemplateErased,
    ) -> (interface::rpc::MessageTemplateErased, u64) {
        assert_eq!(Self::FUNC_ID, req.meta.func_id);
        let conn_id = req.meta.conn_id;
        let call_id = req.meta.call_id;
        let ptr_local =
            req.shm_addr as *mut MessageTemplate<inner::HelloRequest<BackendOwned>, BackendOwned>;
        // TODO(wyj): refine the following line, this pointer may be invalid.
        // should we directly constrct a pointer using remote addr?
        // or just keep the addr u64?
        let ptr_remote = ptr_local.with_addr(req.shm_addr_remote);
        let msg = unsafe { mrpc::alloc::Box::from_backend_raw(ptr_local, ptr_remote) };
        let req = unsafe { mrpc::alloc::Box::from_backend_shmptr(msg.val) };
        // TODO(wyj): lifetime bound for ShmView
        // ShmView should be !Send and !Sync
        let ctx = ();
        let req = ShmView::new_from_backend_owned(req, ShmRecvContext::new(&ctx));
        // TODO(wyj): should not be forget.
        // TODO(wyj): box should differentiate whether the memory is allocated by the app or from the
        // backend's recv_mr. If is from the backend's recv_mr, send a signal to the backend to
        // indicate that we will no longer use the region of this object, so that the backend can
        // do post_recv.
        std::mem::forget(msg);
        match self.inner.say_hello(req) {
            Ok(reply) => {
                reply.inner.meta.conn_id = conn_id;
                reply.inner.meta.call_id = call_id;
                reply.inner.meta.func_id = Self::FUNC_ID;

                let meta = reply.inner.meta;
                // increase send count of RpcMessage
                reply.send_count += 1;

                // TODO(wyj): get rid of this
                reply.switch_address_space();
                let (ptr, ptr_remote) = mrpc::alloc::Box::to_raw_parts(&reply.inner);
                let erased = MessageTemplateErased {
                    meta,
                    shm_addr: ptr_remote.addr().get(),
                    shm_addr_remote: ptr.addr().get(),
                };
                (erased, reply.identifier)
            }
            Err(_status) => {
                todo!();
            }
        }
    }
}
