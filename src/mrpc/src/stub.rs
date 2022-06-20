use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io;
use std::mem::ManuallyDrop;
use std::net::ToSocketAddrs;
use std::ops::Deref;
use std::sync::atomic::{AtomicU64, Ordering};

use arrayvec::ArrayVec;
use fnv::FnvHashMap;

use interface::Handle;
use ipc::mrpc::cmd::{Command, CompletionKind};
use ipc::mrpc::dp;
use ipc::mrpc::dp::RECV_RECLAIM_BS;
use libkoala::_rx_recv_impl as rx_recv_impl;

/// Re-exports
pub use interface::rpc::{MessageErased, MessageMeta, RpcMsgType};
pub use ipc::mrpc::control_plane::TransportType;

use crate::alloc::Box;
use crate::salloc::gc::{
    CS_STUB_ID_COUNTER, GARBAGE_COLLECTOR, MESSAGE_ID_COUNTER, OUTSTANDING_WR,
};
use crate::salloc::region::SharedRecvBuffer;
use crate::salloc::SA_CTX;
use crate::{Error, MRPC_CTX};

/// ReclaimBuffer
#[derive(Debug)]
pub struct ReclaimBuffer(pub(crate) RefCell<ArrayVec<u32, RECV_RECLAIM_BS>>);

impl ReclaimBuffer {
    #[inline]
    fn new() -> Self {
        ReclaimBuffer(RefCell::new(ArrayVec::new()))
    }
}

thread_local! {
    // map reply from conn_id + call_id to MessageErased
    pub(crate) static RECV_REPLY_CACHE: RefCell<FnvHashMap<dp::WrIdentifier, MessageErased>> = RefCell::new(FnvHashMap::default());
    // maintain a per server stub recv buffer
    pub(crate) static RECV_REQUEST_CACHE: RefCell<FnvHashMap<u64, Vec<MessageErased>>> = RefCell::new(FnvHashMap::default());
    // map conn_id to server stub
    pub(crate) static CONN_SERVER_STUB_MAP: RefCell<FnvHashMap<Handle, u64>> = RefCell::new(FnvHashMap::default());
}

pub trait RpcData: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> RpcData for T {}

// NOTE(wyj): if T is not an app owned request/reply
// then RpcMessage<T> cannot be sent via public APIs
// it will automatically be deallocated when it is dropped,
// since send_count is 0
#[derive(Debug)]
pub struct RpcMessage<T: RpcData> {
    pub(crate) inner: ManuallyDrop<Box<T>>,
    // ID of this RpcMessage
    pub(crate) identifier: u64,
    // How many times the message is sent
    pub(crate) send_count: AtomicU64,
}

impl<T: RpcData> RpcMessage<T> {
    pub fn new(msg: T) -> Self {
        let msg = Box::new(msg);
        let message_id = MESSAGE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
        RpcMessage {
            inner: ManuallyDrop::new(msg),
            identifier: message_id,
            send_count: AtomicU64::new(0),
        }
    }
}

impl<T: RpcData> Deref for RpcMessage<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.inner.as_ref()
    }
}

impl<T: RpcData> Drop for RpcMessage<T> {
    fn drop(&mut self) {
        let inner = unsafe { ManuallyDrop::take(&mut self.inner) };
        GARBAGE_COLLECTOR.collect(
            inner,
            self.identifier,
            self.send_count.load(Ordering::Acquire),
        )
    }
}

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::shmview::ShmView;

pub struct ReqFuture<'a, T> {
    wr_id: dp::WrIdentifier,
    reclaim_buffer: &'a ReclaimBuffer,
    _marker: PhantomData<T>,
}

impl<'a, T: Unpin> Future for ReqFuture<'a, T> {
    type Output = Result<ShmView<'a, T>, crate::Status>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        check_completion_queue();
        if let Some(erased) = RECV_REPLY_CACHE.with(|cache| cache.borrow_mut().remove(&this.wr_id))
        {
            tracing::trace!(
                "ReqFuture receive reply from mRPC engine, call_id={}",
                erased.meta.call_id
            );
            let ptr_app = erased.shm_addr_app as *mut T;
            let ptr_backend = ptr_app.with_addr(erased.shm_addr_backend);
            let backend_owned_msg = unsafe { Box::from_raw(ptr_app, ptr_backend) };
            let reply = ShmView::new(backend_owned_msg, this.wr_id, &this.reclaim_buffer);
            Poll::Ready(Ok(reply))
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

pub(crate) fn check_completion_queue() -> Result<(), super::Error> {
    const BUF_LEN: usize = 32;
    let mut buffer = Vec::with_capacity(BUF_LEN);
    MRPC_CTX.with(|ctx| {
        ctx.service.dequeue_wc_with(|ptr, count| unsafe {
            for i in 0..count {
                let c = ptr.add(i).cast::<dp::Completion>().read();
                buffer.push(c);
            }
            count
        })?;
        for c in buffer {
            match c {
                dp::Completion::Recv(msg) => {
                    let conn_id = msg.meta.conn_id;
                    let call_id = msg.meta.call_id;
                    match msg.meta.msg_type {
                        RpcMsgType::Request => {
                            let stub_id = CONN_SERVER_STUB_MAP
                                .with(|map| map.borrow().get(&conn_id).map(|x| *x));
                            if let Some(stub_id) = stub_id {
                                RECV_REQUEST_CACHE.with(|cache| {
                                    if let Some(buffer) = cache.borrow_mut().get_mut(&stub_id) {
                                        buffer.push(msg);
                                    }
                                })
                            }
                        }
                        RpcMsgType::Response => RECV_REPLY_CACHE.with(|cache| {
                            cache
                                .borrow_mut()
                                .insert(dp::WrIdentifier(conn_id, call_id), msg);
                        }),
                    }
                }
                dp::Completion::SendCompletion(dp::WrIdentifier(conn_id, call_id)) => {
                    let msg_id = OUTSTANDING_WR.with(|outstanding| {
                        outstanding
                            .borrow_mut()
                            .remove(&dp::WrIdentifier(conn_id, call_id))
                            .expect("received unrecognized WR completion ACK")
                    });
                    GARBAGE_COLLECTOR.register_wr_completion(msg_id, 1);
                }
            }
        }
        Ok(())
    })
}

#[derive(Debug)]
pub struct ClientStub {
    // identifier for the stub
    stub_id: u64,
    // mRPC connection handle
    handle: Handle,
    reply_mrs: Vec<SharedRecvBuffer>,
    recv_reclaim_buffer: ReclaimBuffer,
}

impl ClientStub {
    pub fn unary<'a, Req, Res>(
        &self,
        service_id: u32,
        func_id: u32,
        call_id: u32,
        msg: &RpcMessage<Req>,
    ) -> impl Future<Output = Result<ShmView<Res>, crate::Status>> + '_
    where
        Req: RpcData,
        Res: Unpin + RpcData,
    {
        let conn_id = self.get_handle();
        // construct meta
        let meta = MessageMeta {
            conn_id,
            service_id,
            func_id,
            call_id,
            len: 0,
            msg_type: RpcMsgType::Request,
        };

        // increase send count for RpcMessage
        msg.send_count.fetch_add(1, Ordering::AcqRel);
        self.post_request(msg, meta).unwrap();

        ReqFuture {
            wr_id: dp::WrIdentifier(conn_id, call_id),
            reclaim_buffer: &self.recv_reclaim_buffer,
            _marker: PhantomData,
        }
    }
}

impl ClientStub {
    #[inline]
    pub fn get_handle(&self) -> Handle {
        self.handle
    }

    pub fn set_transport(transport_type: TransportType) -> Result<(), Error> {
        let req = Command::SetTransport(transport_type);
        MRPC_CTX.with(|ctx| {
            ctx.service.send_cmd(req)?;
            rx_recv_impl!(ctx.service, CompletionKind::SetTransport)?;
            Ok(())
        })
    }

    pub(crate) fn post_request<T: RpcData>(
        &self,
        msg: &RpcMessage<T>,
        meta: MessageMeta,
    ) -> Result<(), Error> {
        tracing::trace!(
            "client post request to mRPC engine, call_id={}",
            meta.call_id
        );

        let (ptr_app, ptr_backend) = Box::to_raw_parts(&msg.inner);
        let erased = MessageErased {
            meta,
            shm_addr_app: ptr_app.addr().get(),
            shm_addr_backend: ptr_backend.addr().get(),
        };
        let req = dp::WorkRequest::Call(erased);

        // codegen should increase send_count for RpcMessage
        OUTSTANDING_WR.with(|outstanding| {
            outstanding
                .borrow_mut()
                .insert(dp::WrIdentifier(meta.conn_id, meta.call_id), msg.identifier);
        });
        MRPC_CTX.with(|ctx| {
            let mut sent = false;
            while !sent {
                ctx.service.enqueue_wr_with(|ptr, _count| unsafe {
                    ptr.cast::<dp::WorkRequest>().write(req);
                    sent = true;
                    1
                })?;
            }
            Ok(())
        })
    }

    pub fn connect<A: ToSocketAddrs>(addr: A) -> Result<Self, Error> {
        let connect_addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or(Error::NoAddrResolved)?;
        let req = Command::Connect(connect_addr);

        MRPC_CTX.with(|ctx| {
            ctx.service.send_cmd(req)?;
            let fds = ctx.service.recv_fd()?;
            rx_recv_impl!(ctx.service, CompletionKind::Connect, ret, {
                use memfd::Memfd;
                assert_eq!(fds.len(), ret.1.len());
                let mut vaddrs = Vec::new();
                let reply_mrs = ret
                    .1
                    .into_iter()
                    .zip(&fds)
                    .map(|(mr, &fd)| {
                        let memfd = Memfd::try_from_fd(fd)
                            .map_err(|_| io::Error::last_os_error())
                            .unwrap();
                        let m = SharedRecvBuffer::new(mr.1, mr.2, mr.3, memfd).unwrap();
                        vaddrs.push((mr.0, m.as_ptr().expose_addr()));
                        m
                    })
                    .collect();
                // return the mapped addr back
                // TODO(cjr): send to SA_CTX
                SA_CTX.with(|sa_ctx| {
                    let req = ipc::salloc::cmd::Command::NewMappedAddrs(vaddrs);
                    sa_ctx.service.send_cmd(req)?;
                    // COMMENT(cjr): must wait for the reply!
                    rx_recv_impl!(
                        sa_ctx.service,
                        ipc::salloc::cmd::CompletionKind::NewMappedAddrs
                    )?;
                    Result::<(), Error>::Ok(())
                })?;

                let stub_id = CS_STUB_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(Self {
                    stub_id,
                    handle: ret.0,
                    reply_mrs,
                    recv_reclaim_buffer: ReclaimBuffer::new(),
                })
            })
        })
    }
}

impl !Send for ClientStub {}
impl !Sync for ClientStub {}

impl Drop for ClientStub {
    fn drop(&mut self) {
        OUTSTANDING_WR.with(|outstanding| {
            let mut borrow = outstanding.borrow_mut();
            let wrs = borrow.drain_filter(|wr_id, _| self.handle == wr_id.0);

            let msg_cnt = wrs.fold(HashMap::new(), |mut acc, (_, msg_id)| {
                *acc.entry(msg_id).or_insert(0u64) += 1;
                acc
            });

            for (msg_id, cnt) in msg_cnt {
                GARBAGE_COLLECTOR.register_wr_completion(msg_id, cnt);
            }
        });
    }
}

pub struct Server {
    stub_id: u64,
    listener_handle: Handle,
    handles: HashSet<Handle>,
    // NOTE(wyj): store recv mrs according to connection handle
    // instead of MR handle
    mrs: HashMap<Handle, Vec<SharedRecvBuffer>>,
    // service_id -> Service
    routes: HashMap<u32, std::boxed::Box<dyn Service>>,
    recv_reclaim_buffer: FnvHashMap<Handle, ReclaimBuffer>,
}

impl Server {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> Result<Self, Error> {
        const RECV_BUF_SIZE: usize = 32;

        let bind_addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or(Error::NoAddrResolved)?;
        let req = Command::Bind(bind_addr);
        MRPC_CTX.with(|ctx| {
            ctx.service.send_cmd(req)?;
            rx_recv_impl!(ctx.service, CompletionKind::Bind, listener_handle, {
                let stub_id = CS_STUB_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                // setup recv cache
                RECV_REQUEST_CACHE.with(|cache| {
                    cache
                        .borrow_mut()
                        .insert(stub_id, Vec::with_capacity(RECV_BUF_SIZE))
                });
                Ok(Self {
                    stub_id,
                    listener_handle,
                    handles: HashSet::default(),
                    mrs: Default::default(),
                    routes: HashMap::default(),
                    recv_reclaim_buffer: FnvHashMap::default(),
                })
            })
        })
    }

    pub fn add_service<S: Service + NamedService + 'static>(&mut self, svc: S) -> &mut Self {
        match self.routes.insert(S::SERVICE_ID, std::boxed::Box::new(svc)) {
            Some(_) => panic!(
                "A func_id can only have 1 handler, func_id: {}",
                S::SERVICE_ID
            ),
            None => {}
        }
        self
    }

    /// Receive data from read shared heap and look up the routes and dispatch the erased message.
    pub fn serve(&mut self) -> Result<(), std::boxed::Box<dyn std::error::Error>> {
        loop {
            // check new incoming connections
            self.check_new_incoming_connection()?;
            // check new requests
            let msgs = self.poll_requests()?;
            self.post_replies(msgs)?;
        }
    }

    fn check_new_incoming_connection(
        &mut self,
    ) -> Result<(), std::boxed::Box<dyn std::error::Error>> {
        MRPC_CTX.with(|ctx| {
            // TODO(cjr): the implementation of this function is super slow
            match ctx.service.try_recv_fd() {
                Ok(fds) => {
                    let mut vaddrs = Vec::new();
                    let mut recv_mrs = Vec::new();
                    rx_recv_impl!(ctx.service, CompletionKind::NewConnection, ret, {
                        use memfd::Memfd;
                        assert_eq!(fds.len(), ret.1.len());
                        assert!(self.handles.insert(ret.0));
                        // setup recv cache and recv reclaim buffer
                        CONN_SERVER_STUB_MAP
                            .with(|map| map.borrow_mut().insert(ret.0, self.stub_id));
                        self.recv_reclaim_buffer.insert(ret.0, ReclaimBuffer::new());
                        for (mr, &fd) in ret.1.into_iter().zip(&fds) {
                            let memfd = Memfd::try_from_fd(fd)
                                .map_err(|_| io::Error::last_os_error())
                                .unwrap();
                            let m = SharedRecvBuffer::new(mr.1, mr.2, mr.3, memfd).unwrap();
                            vaddrs.push((mr.0, m.as_ptr().expose_addr()));
                            recv_mrs.push(m);
                        }
                        self.mrs.insert(ret.0, recv_mrs);
                        Ok(())
                    })?;
                    // return the mapped addr back
                    // TODO(cjr): send to SA_CTX
                    SA_CTX.with(|sa_ctx| {
                        let req = ipc::salloc::cmd::Command::NewMappedAddrs(vaddrs);
                        sa_ctx.service.send_cmd(req)?;
                        // COMMENT(cjr): must wait for the reply!
                        rx_recv_impl!(
                            sa_ctx.service,
                            ipc::salloc::cmd::CompletionKind::NewMappedAddrs
                        )?;
                        Result::<(), Error>::Ok(())
                    })?;
                }
                Err(ipc::Error::TryRecvFd(ipc::TryRecvError::Empty)) => {}
                Err(e) => return Err(e.into()),
            }
            Ok(())
        })
    }

    pub(crate) fn post_reply(&self, erased: MessageErased, msg_id: u64) -> Result<(), Error> {
        tracing::trace!(
            "client post reply to mRPC engine, call_id={}",
            erased.meta.call_id
        );

        let wr = dp::WorkRequest::Reply(erased);

        // codegen should increase send_count for RpcMessage
        let meta = erased.meta;
        OUTSTANDING_WR.with(|outstanding| {
            outstanding
                .borrow_mut()
                .insert(dp::WrIdentifier(meta.conn_id, meta.call_id), msg_id);
        });

        MRPC_CTX.with(|ctx| {
            let mut sent = false;
            while !sent {
                ctx.service.enqueue_wr_with(|ptr, _count| unsafe {
                    ptr.cast::<dp::WorkRequest>().write(wr);
                    sent = true;
                    1
                })?;
            }
            Ok(())
        })
    }

    fn post_replies(
        &mut self,
        msgs: Vec<(MessageErased, u64)>,
    ) -> Result<(), std::boxed::Box<dyn std::error::Error>> {
        for (reply, msg_id) in msgs {
            self.post_reply(reply, msg_id)?;
        }
        Ok(())
    }

    fn poll_requests(
        &mut self,
    ) -> Result<Vec<(MessageErased, u64)>, std::boxed::Box<dyn std::error::Error>> {
        let mut msgs = Vec::with_capacity(32);

        check_completion_queue()?;
        RECV_REQUEST_CACHE.with(|cache| {
            let mut borrow = cache.borrow_mut();

            for request in borrow.get_mut(&self.stub_id).unwrap().drain(..) {
                let service_id = request.meta.service_id;
                match self.routes.get_mut(&service_id) {
                    Some(s) => {
                        let recv_buffer =
                            self.recv_reclaim_buffer.get(&request.meta.conn_id).unwrap();
                        let (reply_erased, msg_id) = s.call(request, recv_buffer);
                        msgs.push((reply_erased, msg_id));
                    }
                    None => {
                        eprintln!("unrecognized request: {:?}", request);
                    }
                }
            }
        });

        Ok(msgs)
    }
}

impl !Send for Server {}
impl !Sync for Server {}

impl Drop for Server {
    fn drop(&mut self) {
        // clear recv cache setup
        CONN_SERVER_STUB_MAP.with(|map| {
            let mut borrow = map.borrow_mut();
            for conn_id in self.handles.iter() {
                borrow.remove(conn_id);
            }
        });
        RECV_REQUEST_CACHE.with(|cache| cache.borrow_mut().remove(&self.stub_id));

        // remove all outstanding WR releated to this client/server stub
        OUTSTANDING_WR.with(|outstanding| {
            let mut borrow = outstanding.borrow_mut();
            let wrs = borrow.drain_filter(|wr_id, _| self.handles.contains(&wr_id.0));

            let msg_cnt = wrs.fold(HashMap::new(), |mut acc, (_, msg_id)| {
                *acc.entry(msg_id).or_insert(0u64) += 1;
                acc
            });

            for (msg_id, cnt) in msg_cnt {
                GARBAGE_COLLECTOR.register_wr_completion(msg_id, cnt);
            }
        });
    }
}

pub trait NamedService {
    const SERVICE_ID: u32;
    const NAME: &'static str = "";
}

pub trait Service {
    // return erased reply and ID of its corresponding RpcMessage
    fn call(&mut self, req: MessageErased, reclaim_buffer: &ReclaimBuffer) -> (MessageErased, u64);
}

pub fn service_pre_handler<'a, T: Unpin>(
    req: &MessageErased,
    reclaim_buffer: &'a ReclaimBuffer,
) -> ShmView<'a, T> {
    let wr_id = dp::WrIdentifier(req.meta.conn_id, req.meta.call_id);
    let ptr_app = req.shm_addr_app as *mut T;
    // TODO(wyj): refine the following line, this pointer may be invalid.
    // should we directly constrct a pointer using remote addr?
    // or just keep the addr u64?
    let ptr_backend = ptr_app.with_addr(req.shm_addr_backend);
    let backend_owned_msg = unsafe { Box::from_raw(ptr_app, ptr_backend) };

    // TODO(wyj): lifetime bound for ShmView
    // ShmView should be !Send and !Sync
    ShmView::new(backend_owned_msg, wr_id, reclaim_buffer)
}

pub fn service_post_handler<T: RpcData>(
    reply: &mut RpcMessage<T>,
    conn_id: Handle,
    service_id: u32,
    func_id: u32,
    call_id: u32,
) -> (MessageErased, u64) {
    // construct meta
    let meta = MessageMeta {
        conn_id,
        service_id,
        func_id,
        call_id,
        len: 0,
        msg_type: RpcMsgType::Response,
    };

    // increase send count of RpcMessage
    reply.send_count.fetch_add(1, Ordering::AcqRel);

    let (ptr_app, ptr_backend) = Box::to_raw_parts(&reply.inner);
    let erased = MessageErased {
        meta,
        shm_addr_app: ptr_app.addr().get(),
        shm_addr_backend: ptr_backend.addr().get(),
    };
    (erased, reply.identifier)
}