use std::net::ToSocketAddrs;
use std::pin::Pin;

use std::future::Future;
use std::rc::Rc;
use std::task::{Context, Poll};

use fnv::FnvHashMap as HashMap;
use ipc::shmalloc::SwitchAddressSpace;

// use crate::mrpc::shmptr::ShmPtr;
use crate::mrpc;
use crate::mrpc::alloc::ShmView;
use crate::mrpc::stub::{
    self, ClientStub, MessageTemplate, MessageTemplateErased, NamedService, Service, RpcMessage
};
use crate::mrpc::MRPC_CTX;
use crate::salloc::owner::{BackendOwned, AppOwned};


// mimic the generated code of tonic-helloworld

// # Safety
//
// The zero-copy inter-process communication thing is beyond what the compiler
// can check. The programmer must ensure that everything is fine.

// Manually write all generated code

pub type HelloRequest = inner::HelloRequest;
pub type HelloReply = inner::HelloReply;


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
                name: mrpc::alloc::Vec::clone_from_backend_owned(&backend_owned.name)
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
                name: mrpc::alloc::Vec::clone_from_backend_owned(&backend_owned.name)
            }
        }
    }
}


pub struct ReqFuture {
    call_id: u64,
    reply_cache: Rc<ReplyCache>,
}

impl Future for ReqFuture {
    type Output = Result<ShmView<HelloReply>, mrpc::Status>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        check_completion(&*this.reply_cache).unwrap();
        if let Some(erased) = this.reply_cache.remove(this.call_id) {
            tracing::trace!("ReqFuture receive reply from mRPC engine, call_id={}", erased.meta.call_id);
            let raw = erased.shm_addr as *mut MessageTemplate<inner::HelloReply<BackendOwned>>;
            let msg = unsafe { mrpc::alloc::Box::from_backend_raw(raw, erased.shm_addr_remote) };
            let reply = unsafe { mrpc::alloc::Box::from_backend_shmptr(msg.val) };
            let reply = ShmView::new_from_backend_owned(reply);
            Poll::Ready(Ok(reply))
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

fn check_completion(reply_cache: &ReplyCache) -> Result<(), super::Error> {
    use ipc::mrpc::dp;
    let mut msgs = Vec::with_capacity(32);
    MRPC_CTX.with(|ctx| {
        ctx.service.dequeue_wc_with(|ptr, count| unsafe {
            for i in 0..count {
                let c = ptr.add(i).cast::<dp::Completion>().read();
                msgs.push(c.erased);
            }
            count
        })?;
        for m in msgs {
            let call_id = m.meta.call_id;
            reply_cache.insert(call_id, m);
        }
        Ok(())
    })
}

// Reply cache, call_id -> Reply, Sync, not durable
#[derive(Debug)]
pub struct ReplyCache {
    cache: spin::Mutex<HashMap<u64, MessageTemplateErased>>,
}

impl ReplyCache {
    fn new() -> Self {
        Self {
            cache: spin::Mutex::new(HashMap::default()),
        }
    }

    #[inline]
    fn insert(&self, call_id: u64, erased: MessageTemplateErased) {
        self.cache
            .lock()
            .insert(call_id, erased)
            .ok_or(())
            .unwrap_err();
    }

    #[inline]
    fn remove(&self, call_id: u64) -> Option<MessageTemplateErased> {
        self.cache.lock().remove(&call_id)
    }
}

#[derive(Debug)]
pub struct GreeterClient {
    pub stub: ClientStub,
    // call_id -> ShmBox<Reply>
    reply_cache: Rc<ReplyCache>,
    pub call_counter: u64,
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
            reply_cache: Rc::new(ReplyCache::new()),
            call_counter: 0,
        })
    }

    pub fn say_hello(
        &mut self,
        msg: &mut RpcMessage<inner::HelloRequest<AppOwned>>,
    ) -> impl Future<Output = Result<ShmView<HelloReply>, mrpc::Status>> {
        let call_id = self.call_counter;
        msg.inner.meta.conn_id = self.stub.get_handle();
        msg.inner.meta.call_id = call_id;
        msg.inner.meta.func_id = Self::FUNC_ID;

        self.call_counter += 1;

        stub::post_request(&mut msg.inner).unwrap();
        ReqFuture {
            call_id,
            reply_cache: self.reply_cache.clone(),
        }
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
    ) -> interface::rpc::MessageTemplateErased {
        assert_eq!(Self::FUNC_ID, req.meta.func_id);
        let conn_id = req.meta.conn_id;
        let call_id = req.meta.call_id;
        let raw = req.shm_addr as *mut MessageTemplate<inner::HelloRequest<BackendOwned>>;
        // TODO(wyj): refine the following line, this pointer may be invalid.
        // should we directly constrct a pointer using remote addr?
        // or just keep the addr u64?
        let addr_remote = req.shm_addr_remote;
        let msg = unsafe { mrpc::alloc::Box::from_backend_raw(raw, addr_remote) };
        let req = unsafe { mrpc::alloc::Box::from_backend_shmptr(msg.val) };
        let req = ShmView::new_from_backend_owned(req);
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
                reply.switch_address_space();
                let (ptr, addr_remote) = mrpc::alloc::Box::as_ptr(&reply.inner);
                let erased = MessageTemplateErased {
                    meta,
                    shm_addr: addr_remote,
                    shm_addr_remote: ptr as *const () as usize
                };
                erased
            }
            Err(_status) => {
                todo!();
            }
        }
    }
}
