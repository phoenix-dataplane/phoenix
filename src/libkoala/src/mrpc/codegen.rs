use std::net::ToSocketAddrs;
use std::pin::Pin;

use std::future::Future;
use std::task::{Context, Poll};

use fnv::FnvHashMap as HashMap;
use unique::Unique;

// use crate::mrpc::shmptr::ShmPtr;
use crate::mrpc;
use crate::mrpc::shared_heap::SharedHeapAllocator;
use crate::mrpc::stub::{
    self, ClientStub, MessageTemplate, MessageTemplateErased, NamedService, Service,
};
use crate::mrpc::MRPC_CTX;

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
#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct HelloReply {
    pub name: mrpc::alloc::Vec<u8>, // change to mrpc::alloc::Vec<u8>, -> String
}

unsafe impl SwitchAddressSpace for HelloReply {
    fn switch_address_space(&mut self) {
        self.name.switch_address_space();
    }
}

pub struct ReqFuture<'a> {
    call_id: u64,
    reply_cache: &'a ReplyCache,
}

impl<'a> Future for ReqFuture<'a> {
    type Output = Result<HelloReply, mrpc::Status>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        check_completion(&this.reply_cache).unwrap();
        if let Some(erased) = this.reply_cache.remove(this.call_id) {
            // unmarshal
            let msg: Unique<MessageTemplate<HelloReply>> =
                Unique::new(erased.shmptr as *mut _).unwrap();
            // TODO(cjr): when to drop the reply
            let msg = unsafe { msg.as_ref().val.as_ref().clone() };
            Poll::Ready(Ok(msg))
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
    stub: ClientStub,
    // call_id -> ShmBox<Reply>
    reply_cache: ReplyCache,
    call_counter: u64,
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
            reply_cache: ReplyCache::new(),
            call_counter: 0,
        })
    }

    pub fn say_hello(
        &mut self,
        request: mrpc::alloc::Box<HelloRequest>,
    ) -> impl Future<Output = Result<HelloReply, mrpc::Status>> + '_ {
        let msg = MessageTemplate::new(
            request,
            self.stub.get_handle(),
            Self::FUNC_ID,
            self.call_counter,
        );
        self.call_counter += 1;
        let call_id = msg.meta.call_id;
        stub::post_request(msg).unwrap();
        ReqFuture {
            call_id,
            reply_cache: &self.reply_cache,
        }
    }
}

impl NamedService for GreeterClient {
    const FUNC_ID: u32 = 0;
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
        let call_id = req.meta.call_id;
        let raw = req.shmptr as *mut MessageTemplate<HelloRequest>;
        let msg = unsafe { mrpc::alloc::Box::from_raw_in(raw, SharedHeapAllocator) };
        let req = unsafe { mrpc::alloc::Box::from_raw_in(msg.val.as_ptr(), SharedHeapAllocator) };
        std::mem::forget(msg);
        match self.inner.say_hello(req) {
            Ok(reply) => {
                let mut msg = MessageTemplate::new_reply(reply, conn_id, Self::FUNC_ID, call_id);
                let meta = msg.meta;
                msg.switch_address_space();
                let local_ptr = mrpc::alloc::Box::into_raw(msg);
                let remote_shmptr = local_ptr
                    .cast::<u8>()
                    .wrapping_offset(SharedHeapAllocator::query_shm_offset(local_ptr as _))
                    as u64;
                let erased = MessageTemplateErased {
                    meta,
                    shmptr: remote_shmptr,
                };
                erased
            }
            Err(_status) => {
                todo!();
            }
        }
    }
}
