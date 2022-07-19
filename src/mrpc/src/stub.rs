use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::net::ToSocketAddrs;
use std::sync::atomic::Ordering;

use dashmap::DashMap;
use fnv::FnvHashMap;

use interface::rpc::{RpcId, TransportStatus};
use interface::{AsHandle, Handle};
use ipc::mrpc::cmd::{Command, CompletionKind, ConnectResponse};
use ipc::mrpc::dp;
use libkoala::_rx_recv_impl as rx_recv_impl;

/// Re-exports
pub use interface::rpc::{MessageErased, MessageMeta, RpcMsgType};
pub use ipc::mrpc::control_plane::TransportType;

use crate::rref::RRef;
use crate::salloc::gc::CS_STUB_ID_COUNTER;
use crate::salloc::ReadHeap;
use crate::wref::{WRef, WRefOpaque};
use crate::{Error, Status, MRPC_CTX};

#[derive(Debug, Default)]
struct PendingWRef {
    pool: DashMap<RpcId, WRefOpaque, fnv::FnvBuildHasher>,
}

impl PendingWRef {
    #[inline]
    fn new() -> Self {
        Self::default()
    }

    #[inline]
    fn insert<T: RpcData>(&self, rpc_id: RpcId, wref: WRef<T>) {
        self.pool.insert(rpc_id, wref.into_opaque());
    }

    #[inline]
    fn remove(&self, rpc_id: &RpcId) {
        self.pool.remove(rpc_id);
    }

    /// Erase all the pending messages from a given connection.
    #[inline]
    fn erase_by_connection(&self, conn: Handle) {
        self.pool.retain(|rpc_id, _| rpc_id.0 != conn);
    }

    fn erase_by(&self, f: impl FnMut(&RpcId, &mut WRefOpaque) -> bool) {
        self.pool.retain(f);
    }
}

lazy_static::lazy_static! {
    static ref PENDING_WREF: PendingWRef = PendingWRef::new();

    // maintain a per server stub recv buffer
    // stub_id -> queue of incoming Messages
    pub(crate) static ref RECV_REQUEST_CACHE: DashMap<u64, Vec<MessageErased>, fnv::FnvBuildHasher> = DashMap::default();
}

thread_local! {
    // map reply from conn_id + call_id to MessageErased
    pub(crate) static RECV_REPLY_CACHE: RefCell<FnvHashMap<RpcId, Result<MessageErased, TransportStatus>>> = RefCell::new(FnvHashMap::default());
    // map conn_id to server stub
    pub(crate) static CONN_SERVER_STUB_MAP: RefCell<FnvHashMap<Handle, u64>> = RefCell::new(FnvHashMap::default());
}

// We can make RpcData a private trait, and only mark it for compiler generated types.
// This seems impossible.
pub trait RpcData: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> RpcData for T {}

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct ReqFuture<'a, T> {
    rpc_id: RpcId,
    read_heap: &'a ReadHeap,
    _marker: PhantomData<T>,
}

impl<'a, T: Unpin> Future for ReqFuture<'a, T> {
    type Output = Result<RRef<'a, T>, Status>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        // handle the warning here
        let _ = check_completion_queue();

        tracing::trace!(
            "ReqFuture receive reply from mRPC engine, rpc_id={:?}",
            this.rpc_id
        );
        match RECV_REPLY_CACHE.with(|cache| cache.borrow_mut().remove(&this.rpc_id)) {
            None => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Some(Ok(erased)) => {
                let reply = RRef::new(&erased, this.read_heap);
                Poll::Ready(Ok(reply))
            }
            Some(Err(status)) => Poll::Ready(Err(Status::from_incoming_transport(status))),
        }
    }
}

thread_local! {
    static COMP_READ_BUFFER: RefCell<Vec<dp::Completion>> = RefCell::new(Vec::with_capacity(32));
}

pub(crate) fn check_completion_queue() -> Result<(), super::Error> {
    MRPC_CTX.with(|ctx| {
        COMP_READ_BUFFER.with_borrow_mut(|buffer| {
            buffer.clear();

            ctx.service.dequeue_wc_with(|ptr, count| unsafe {
                for i in 0..count {
                    let c = ptr.add(i).cast::<dp::Completion>().read();
                    buffer.push(c);
                }
                count
            })?;

            for c in buffer {
                match c {
                    dp::Completion::Incoming(msg, status) => {
                        let conn_id = msg.meta.conn_id;
                        let call_id = msg.meta.call_id;
                        match msg.meta.msg_type {
                            RpcMsgType::Request => {
                                // server receives requests
                                let stub_id = CONN_SERVER_STUB_MAP
                                    .with(|map| map.borrow().get(&conn_id).map(|x| *x));
                                if let Some(stub_id) = stub_id {
                                    RECV_REQUEST_CACHE
                                        .entry(stub_id)
                                        .and_modify(|b| b.push(*msg));
                                }
                            }
                            RpcMsgType::Response => {
                                // client receives responses
                                RECV_REPLY_CACHE.with_borrow_mut(|cache| match *status {
                                    TransportStatus::Success => {
                                        cache.insert(RpcId(conn_id, call_id), Ok(*msg));
                                    }
                                    TransportStatus::Error(_e) => {
                                        cache.insert(RpcId(conn_id, call_id), Err(*status));
                                    }
                                })
                            }
                        }
                    }
                    dp::Completion::Outgoing(rpc_id, status) => {
                        PENDING_WREF.remove(rpc_id);

                        // bubble the error up to the user for RpcRequest
                        // TODO(cjr): fix the problem here
                        if let TransportStatus::Error(_) = *status {
                            RECV_REPLY_CACHE
                                .with_borrow_mut(|cache| cache.insert(*rpc_id, Err(*status)));
                        }
                    }
                }
            }
            Ok(())
        })
    })
}

#[derive(Debug)]
pub struct ClientStub {
    // mRPC connection handle
    handle: Handle,
    #[allow(unused)]
    read_heap: ReadHeap,
}

impl ClientStub {
    pub fn unary<'a, Req, Res>(
        &'a self,
        service_id: u32,
        func_id: u32,
        call_id: u32,
        req: WRef<Req>,
    ) -> impl Future<Output = Result<RRef<'a, Res>, Status>> + '_
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
            token: req.token().0 as u64,
            msg_type: RpcMsgType::Request,
        };

        self.post_request(req, meta).unwrap();

        ReqFuture {
            rpc_id: RpcId(conn_id, call_id),
            read_heap: &self.read_heap,
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
        msg: WRef<T>,
        meta: MessageMeta,
    ) -> Result<(), Error> {
        tracing::trace!(
            "client post request to mRPC engine, call_id={}",
            meta.call_id
        );

        // track the msg as pending
        PENDING_WREF.insert(RpcId::new(meta.conn_id, meta.call_id), WRef::clone(&msg));

        let (ptr_app, ptr_backend) = msg.into_shmptr().to_raw_parts();
        let erased = MessageErased {
            meta,
            shm_addr_app: ptr_app.addr().get(),
            shm_addr_backend: ptr_backend.addr().get(),
        };

        let req = dp::WorkRequest::Call(erased);
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

    // TODO(cjr): Change this to async too
    pub fn connect<A: ToSocketAddrs>(addr: A) -> Result<Self, Error> {
        let connect_addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or(Error::NoAddrResolved)?;
        let req = Command::Connect(connect_addr);

        MRPC_CTX.with(|ctx| {
            ctx.service.send_cmd(req)?;
            let fds = ctx.service.recv_fd()?;
            rx_recv_impl!(ctx.service, CompletionKind::Connect, conn_resp, {
                // use memfd::Memfd;
                assert_eq!(fds.len(), conn_resp.read_regions.len());

                let conn_handle = conn_resp.conn_handle;

                let read_heap = ReadHeap::new(&conn_resp, &fds);
                let vaddrs = read_heap
                    .rbufs
                    .iter()
                    .map(|rbuf| (rbuf.as_handle(), rbuf.as_ptr().expose_addr()))
                    .collect();

                // return the mapped addr back
                let req = Command::NewMappedAddrs(conn_handle, vaddrs);
                ctx.service.send_cmd(req)?;
                // wait for the reply!
                rx_recv_impl!(ctx.service, CompletionKind::NewMappedAddrs)?;

                Ok(Self {
                    handle: conn_handle,
                    read_heap,
                })
            })
        })
    }
}

impl !Send for ClientStub {}
impl !Sync for ClientStub {}

impl Drop for ClientStub {
    fn drop(&mut self) {
        PENDING_WREF.erase_by_connection(self.handle);
    }
}

pub struct Server {
    stub_id: u64,
    #[allow(unused)]
    listener_handle: Handle,
    handles: HashSet<Handle>,
    // conn -> ReadHeap
    read_heaps: HashMap<Handle, ReadHeap>,
    // service_id -> Service
    routes: HashMap<u32, Box<dyn Service>>,
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
                let stub_id = CS_STUB_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
                // setup recv cache
                RECV_REQUEST_CACHE.insert(stub_id, Vec::with_capacity(RECV_BUF_SIZE));
                Ok(Self {
                    stub_id,
                    listener_handle,
                    handles: HashSet::default(),
                    read_heaps: Default::default(),
                    routes: HashMap::default(),
                })
            })
        })
    }

    pub fn add_service<S: Service + NamedService + 'static>(&mut self, svc: S) -> &mut Self {
        match self.routes.insert(S::SERVICE_ID, Box::new(svc)) {
            Some(_) => panic!("Hash collisions in func_id: {}", S::SERVICE_ID),
            None => {}
        }
        self
    }

    /// Receive data from read shared heap and look up the routes and dispatch the erased message.
    pub async fn serve(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut msg_buffer = Vec::with_capacity(32);
        loop {
            // TODO(cjr): change this to check_cm_event(); cm event contains new connections
            // establishment and destruction; read heap scaling.
            self.check_cm_event()?;
            // check new requests
            self.poll_requests(&mut msg_buffer).await?;
            self.post_replies(&mut msg_buffer)?;
        }

        // use futures::select;
        // use futures::stream::UnorderedFutures;
        // let mut running = UnorderedFutures::new();
        // loop {
        //     select! {
        //         reply_erased = running.next() => {
        //             msg_buffer.push(reply_erased);
        //         }
        //         completed => {}
        //         default => {
        //             // no futures is ready
        //             self.check_cm_event()?;
        //             // check new requests
        //             // self.poll_requests(&mut msg_buffer).await?;
        //             self.poll_requests(&mut running)?;
        //             if !msg_buffer.is_empty() {
        //                 self.post_replies(&mut msg_buffer)?;
        //             }
        //         }
        //     }
        // }
    }

    fn handle_new_connection(
        &mut self,
        conn_resp: ConnectResponse,
        ctx: &crate::Context,
    ) -> Result<(), Error> {
        match ctx.service.recv_fd() {
            Ok(fds) => {
                let conn_handle = conn_resp.conn_handle;
                assert_eq!(fds.len(), conn_resp.read_regions.len());
                assert!(self.handles.insert(conn_handle));
                // setup recv cache
                CONN_SERVER_STUB_MAP.with(|map| map.borrow_mut().insert(conn_handle, self.stub_id));

                let read_heap = ReadHeap::new(&conn_resp, &fds);
                let vaddrs = read_heap
                    .rbufs
                    .iter()
                    .map(|rbuf| (rbuf.as_handle(), rbuf.as_ptr().expose_addr()))
                    .collect();
                self.read_heaps.insert(conn_handle, read_heap);

                // update backend addr mapping
                let req = Command::NewMappedAddrs(conn_handle, vaddrs);
                ctx.service.send_cmd(req)?;
                // NO NEED TO WAIT
                Ok(())
            }
            Err(e) => return Err(e.into()),
        }
    }

    fn check_cm_event(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        MRPC_CTX.with(|ctx| {
            match ctx.service.try_recv_comp().map(|comp| comp.0) {
                Err(ipc::Error::TryRecv(ipc::TryRecvError::Empty)) => {}
                Err(e) => return Err(e.into()),
                Ok(compkind) => {
                    match compkind {
                        Ok(CompletionKind::NewConnection(conn_resp)) => {
                            self.handle_new_connection(conn_resp, ctx)?;
                        }
                        Ok(CompletionKind::NewMappedAddrs) => {
                            // do nothing, just consume the completion
                        }
                        Err(e) => return Err(e.into()),
                        otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
                    }
                }
            }
            Ok(())
        })
    }

    pub(crate) fn post_reply(&self, erased: MessageErased) -> Result<(), Error> {
        tracing::trace!(
            "client post reply to mRPC engine, call_id={}",
            erased.meta.call_id
        );

        let wr = dp::WorkRequest::Reply(erased);
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
        msg_buffer: &mut Vec<MessageErased>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let replies = msg_buffer.drain(..);
        for reply in replies {
            self.post_reply(reply)?;
        }
        Ok(())
    }

    async fn poll_requests(
        &mut self,
        msg_buffer: &mut Vec<MessageErased>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        check_completion_queue()?;

        for request in RECV_REQUEST_CACHE.get_mut(&self.stub_id).unwrap().drain(..) {
            let service_id = request.meta.service_id;
            let read_heap = &self.read_heaps[&request.meta.conn_id];
            match self.routes.get_mut(&service_id) {
                Some(s) => {
                    let reply_erased = s.call(request, read_heap).await;
                    msg_buffer.push(reply_erased);
                }
                None => {
                    eprintln!("unrecognized request: {:?}", request);
                }
            }
        }

        Ok(())
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

        RECV_REQUEST_CACHE.remove(&self.stub_id);

        // remove all outstanding RPC releated to this client/server stub
        PENDING_WREF.erase_by(|rpc_id, _| self.handles.contains(&rpc_id.0));
    }
}

pub trait NamedService {
    const SERVICE_ID: u32;
    const NAME: &'static str = "";
}

#[crate::async_trait]
pub trait Service {
    // return erased reply and ID of its corresponding RpcMessage
    async fn call(&self, req: MessageErased, read_heap: &ReadHeap) -> MessageErased;
}

pub fn service_pre_handler<'a, T: Unpin>(
    req: &MessageErased,
    read_heap: &'a ReadHeap,
) -> RRef<'a, T> {
    RRef::new(&req, read_heap)
}

pub fn service_post_handler<T: RpcData>(
    reply: WRef<T>,
    conn_id: Handle,
    service_id: u32,
    func_id: u32,
    call_id: u32,
) -> MessageErased {
    // construct meta
    let meta = MessageMeta {
        conn_id,
        service_id,
        func_id,
        call_id,
        token: reply.token().0 as u64,
        msg_type: RpcMsgType::Response,
    };

    // track the msg as pending
    PENDING_WREF.insert(RpcId::new(conn_id, func_id), WRef::clone(&reply));

    let (ptr_app, ptr_backend) = reply.into_shmptr().to_raw_parts();
    let erased = MessageErased {
        meta,
        shm_addr_app: ptr_app.addr().get(),
        shm_addr_backend: ptr_backend.addr().get(),
    };

    erased
}

pub fn update_protos(protos: &[&str]) -> Result<(), Error> {
    MRPC_CTX.with(|ctx| ctx.update_protos(protos))
}
