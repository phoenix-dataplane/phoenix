use std::cell::RefCell;
use std::collections::VecDeque;
use std::net::ToSocketAddrs;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use fnv::FnvHashMap as HashMap;
use futures::select;
use futures::stream::{FuturesUnordered, StreamExt};
use futures::task::LocalFutureObj;
use futures::FutureExt;

use interface::rpc::{RpcId, TransportStatus};
use interface::{AsHandle, Handle};
use ipc::mrpc::cmd::{Command, CompletionKind, ConnectResponse};
use ipc::mrpc::dp;
use libkoala::_rx_recv_impl as rx_recv_impl;

/// Re-exports
pub use interface::rpc::{MessageErased, MessageMeta, RpcMsgType};
pub use ipc::mrpc::control_plane::TransportType;

use crate::rref::RRef;
use crate::wref::{WRef, WRefOpaque};
use crate::ReadHeap;
use crate::{Error, Status, MRPC_CTX};

static CS_STUB_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

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
    pub(crate) static RECV_REPLY_CACHE: RefCell<HashMap<RpcId, Result<MessageErased, TransportStatus>>> = RefCell::new(HashMap::default());
    // map conn_id to server stub
    pub(crate) static CONN_SERVER_STUB_MAP: RefCell<HashMap<Handle, u64>> = RefCell::new(HashMap::default());
    // conn_id -> Error
    pub(crate) static CONN_ERROR_BUFFER: RefCell<HashMap<Handle, VecDeque<Error>>> = RefCell::new(HashMap::default());
}

// We can make RpcData a private trait, and only mark it for compiler generated types.
// This seems impossible.
pub trait RpcData: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> RpcData for T {}

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use super::future::yield_now;

pub struct ReqFuture<'a, T> {
    rpc_id: RpcId,
    read_heap: &'a ReadHeap,
    _marker: PhantomData<T>,
}

impl<'a, T: Unpin> Future for ReqFuture<'a, T> {
    type Output = Result<RRef<'a, T>, Status>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        match check_completion_queue() {
            Ok(_) => {}
            Err(Error::Disconnect(_)) => {}
            Err(e) => return Poll::Ready(Err(e.into())),
        }

        let conn_id = this.rpc_id.0;

        // TODO(cjr): For client, you only need to check your own position. No need to read a
        // hashmap each time. This a newly introduced overhead.
        if let Some(err) = CONN_ERROR_BUFFER.with_borrow_mut(|m| {
            m.entry(conn_id)
                .or_insert_with(|| VecDeque::new())
                .pop_front()
        }) {
            return Poll::Ready(Err(err.into()));
        }

        match RECV_REPLY_CACHE.with(|cache| cache.borrow_mut().remove(&this.rpc_id)) {
            None => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            Some(Ok(erased)) => {
                tracing::trace!(
                    "ReqFuture receive reply from mRPC engine, rpc_id={:?}",
                    this.rpc_id
                );
                let reply = RRef::new(&erased, this.read_heap);
                Poll::Ready(Ok(reply))
            }
            Some(Err(status)) => Poll::Ready(Err(Status::from_incoming_transport(status))),
        }
    }
}

#[cfg(feature = "timing")]
use crate::timing::{SampleKind, Timer};

thread_local! {
    static COMP_READ_BUFFER: RefCell<Vec<dp::Completion>> = RefCell::new(Vec::with_capacity(32));

    #[cfg(feature = "timing")]
    static TIMER: RefCell<Timer> = RefCell::new(Timer::new());
}

pub(crate) fn check_completion_queue() -> Result<usize, Error> {
    MRPC_CTX.with(|ctx| {
        let notified = ctx.service.wait_wc(Some(Duration::from_micros(1000)))?; // try 0
        if !notified {
            return Ok(0);
        }

        COMP_READ_BUFFER.with_borrow_mut(|buffer| {
            // SAFETY: dp::Completion is Copy and zerocopy
            unsafe {
                buffer.set_len(0);
            }

            ctx.service
                .dequeue_wc_with(|ptr, count| unsafe {
                    for i in 0..count {
                        let c = ptr.add(i).cast::<dp::Completion>().read();
                        buffer.push(c);
                    }
                    count
                })
                .map_err(Error::Service)?;

            let cnt = buffer.len();

            for c in buffer {
                match c {
                    dp::Completion::Incoming(msg) => {
                        let conn_id = msg.meta.conn_id;
                        let call_id = msg.meta.call_id;
                        match msg.meta.msg_type {
                            RpcMsgType::Request => {
                                // server receives requests
                                #[cfg(feature = "timing")]
                                TIMER.with_borrow_mut(|timer| {
                                    timer
                                        .sample(RpcId(conn_id, call_id), SampleKind::ServerRequest);
                                });
                                let stub_id = CONN_SERVER_STUB_MAP
                                    .with(|map| map.borrow().get(&conn_id).map(|x| *x));
                                if let Some(stub_id) = stub_id {
                                    RECV_REQUEST_CACHE
                                        .entry(stub_id)
                                        .and_modify(|b| b.push(*msg));
                                }
                            }
                            RpcMsgType::Response => {
                                #[cfg(feature = "timing")]
                                TIMER.with_borrow_mut(|timer| {
                                    timer.sample(RpcId(conn_id, call_id), SampleKind::ClientReply);
                                });
                                // client receives responses
                                RECV_REPLY_CACHE.with_borrow_mut(|cache| {
                                    cache.insert(RpcId(conn_id, call_id), Ok(*msg))
                                });
                            }
                        }
                    }
                    // NOTE(cjr): The term Outgoing might be misleading.
                    // If the backend encounters any transport error, the backend also reports the
                    // error through this branch.
                    // Maybe rewrite this.
                    dp::Completion::Outgoing(rpc_id, status) => {
                        PENDING_WREF.remove(rpc_id);

                        // bubble the error up to the user for RpcRequest
                        // TODO(cjr): fix the problem here
                        if let TransportStatus::Error(_) = *status {
                            RECV_REPLY_CACHE
                                .with_borrow_mut(|cache| cache.insert(*rpc_id, Err(*status)));
                        }
                    }
                    dp::Completion::RecvError(conn_id, status) => {
                        // On recv error, the peer probably disconnects.
                        // The server/client should release the related resources of this
                        // connection.
                        // log::debug!("RecvError conn_id: {:?}, status: {:?}", conn_id, status);
                        log::debug!(
                            "Connection {:?} disconnected, status: {:?}",
                            conn_id,
                            status
                        );
                        CONN_ERROR_BUFFER.with_borrow_mut(|m| {
                            m.entry(*conn_id)
                                .or_insert_with(|| VecDeque::new())
                                .push_back(Error::Disconnect(*conn_id))
                        });

                        #[cfg(feature = "timing")]
                        TIMER.with_borrow(|timer| {
                            println!("{}", timer);
                        });
                        return Err(Error::Disconnect(*conn_id));
                    }
                }
            }
            Ok(cnt)
        })
    })
}

/// A connection resource is a Handle plus a ReadHeap
#[derive(Debug)]
struct Connection {
    // mRPC connection handle
    handle: Handle,
    read_heap: Arc<ReadHeap>,
}

impl PartialEq for Connection {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl Eq for Connection {}

impl std::hash::Hash for Connection {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.handle.hash(state)
    }
}

impl Connection {
    #[inline]
    fn new(handle: Handle, read_heap: ReadHeap) -> Self {
        Self {
            handle,
            read_heap: Arc::new(read_heap),
        }
    }
}

#[derive(Debug)]
pub struct ClientStub {
    conn: Connection,
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
            read_heap: &self.conn.read_heap,
            _marker: PhantomData,
        }
    }
}

impl ClientStub {
    #[inline]
    pub fn get_handle(&self) -> Handle {
        self.conn.handle
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

        #[cfg(feature = "timing")]
        TIMER.with_borrow_mut(|timer| {
            timer.sample(
                RpcId::new(meta.conn_id, meta.call_id),
                SampleKind::ClientRequest,
            );
        });

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
                    conn: Connection::new(conn_handle, read_heap),
                })
            })
        })
    }
}

pub struct Pending<T> {
    _data: std::marker::PhantomData<fn() -> T>,
}

pub fn pending<T>() -> Pending<T> {
    Pending {
        _data: std::marker::PhantomData,
    }
}

impl<T> Future for Pending<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl !Send for ClientStub {}
impl !Sync for ClientStub {}

impl Drop for ClientStub {
    fn drop(&mut self) {
        PENDING_WREF.erase_by_connection(self.get_handle());

        #[cfg(feature = "timing")]
        TIMER.with_borrow(|timer| {
            println!("{}", timer);
        });
    }
}

// TODO(cjr): rename it to LocalServer
pub struct Server {
    stub_id: u64,
    #[allow(unused)]
    listener_handle: Handle,
    connections: RefCell<HashMap<Handle, Connection>>,
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
                    connections: RefCell::new(HashMap::default()),
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
    pub async fn serve(&mut self) -> Result<(), Error> {
        // let mut msg_buffer = Vec::with_capacity(32);
        // loop {
        //     // TODO(cjr): change this to check_cm_event(); cm event contains new connections
        //     // establishment and destruction; read heap scaling.
        //     self.check_cm_event()?;
        //     // check new requests
        //     self.poll_requests(&mut msg_buffer).await?;
        //     self.post_replies(&mut msg_buffer)?;
        // }

        // running tasks
        let mut running = FuturesUnordered::new();
        running.push(LocalFutureObj::new(Box::new(std::future::pending())));
        // running.push(LocalFutureObj::new(Box::new(pending())));
        // batching reply small requests for better CPU efficiency
        let mut reply_buffer = Vec::with_capacity(32);
        loop {
            select! {
                reply_erased = running.next() => {
                    if reply_erased.is_none() { continue; }
                    reply_buffer.push(reply_erased.unwrap());
                }
                complete => {
                    panic!("unexpected complete")
                }
                default => {
                    if !reply_buffer.is_empty() {
                        self.post_replies(&mut reply_buffer)?;
                    }
                    // no futures is ready
                    self.check_cm_event()?;
                    // check new requests, dispatch them to the executor
                    self.dispatch_requests(&mut running)?;
                }
            }
            yield_now().await;
        }
    }

    pub async fn serve_with_graceful_shutdown<F>(&mut self, shutdown: F) -> Result<(), Error>
    where
        F: Future<Output = ()> + Unpin,
    {
        let mut shutdown = shutdown.fuse();

        let mut running = FuturesUnordered::new();
        running.push(LocalFutureObj::new(Box::new(std::future::pending())));
        // running.push(LocalFutureObj::new(Box::new(pending())));
        // batching reply small requests for better CPU efficiency
        let mut reply_buffer = Vec::with_capacity(32);
        loop {
            select! {
                reply_erased = running.next() => {
                    if reply_erased.is_none() { continue; }
                    reply_buffer.push(reply_erased.unwrap());
                }
                _ = shutdown => {
                    break;
                },
                complete => {
                    panic!("unexpected complete")
                }
                default => {
                    if !reply_buffer.is_empty() {
                        self.post_replies(&mut reply_buffer)?;
                    }
                    // no futures is ready
                    self.check_cm_event()?;
                    // check new requests, dispatch them to the executor
                    self.dispatch_requests(&mut running)?;
                }
            }
            yield_now().await;
        }
        Ok(())
    }

    fn handle_new_connection(
        &self,
        conn_resp: ConnectResponse,
        ctx: &crate::Context,
    ) -> Result<(), Error> {
        match ctx.service.recv_fd() {
            Ok(fds) => {
                let conn_handle = conn_resp.conn_handle;
                assert_eq!(fds.len(), conn_resp.read_regions.len());
                // setup recv cache
                CONN_SERVER_STUB_MAP.with(|map| map.borrow_mut().insert(conn_handle, self.stub_id));

                let read_heap = ReadHeap::new(&conn_resp, &fds);
                let vaddrs = read_heap
                    .rbufs
                    .iter()
                    .map(|rbuf| (rbuf.as_handle(), rbuf.as_ptr().expose_addr()))
                    .collect();
                // self.read_heaps.insert(conn_handle, read_heap);

                assert_eq!(
                    self.connections
                        .borrow_mut()
                        .insert(conn_handle, Connection::new(conn_handle, read_heap)),
                    None
                );

                // update backend addr mapping
                let req = Command::NewMappedAddrs(conn_handle, vaddrs);
                ctx.service.send_cmd(req)?;
                // NO NEED TO WAIT
                Ok(())
            }
            Err(e) => return Err(e.into()),
        }
    }

    fn close_connection(&self, conn_id: Handle) -> Result<(), Error> {
        // handles
        // read_heaps
        // CONN_SERVER_STUB_MAP
        // RECV_REQUEST_CACHE
        // PENDING_WREF
        self.connections.borrow_mut().remove(&conn_id);

        CONN_SERVER_STUB_MAP.with_borrow_mut(|map| {
            map.remove(&conn_id);
        });

        // remove all outstanding RPC releated to this client/server stub
        PENDING_WREF.erase_by(|rpc_id, _| self.connections.borrow().contains_key(&rpc_id.0));
        Ok(())
    }

    fn check_cm_event(&self) -> Result<(), Error> {
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
                        Err(e) => return Err(Error::Interface("check_cm_event", e)),
                        otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
                    }
                }
            }
            Ok(())
        })
    }

    fn post_replies(&self, msg_buffer: &mut Vec<MessageErased>) -> Result<(), Error> {
        let num = msg_buffer.len();
        let mut sent = 0;
        MRPC_CTX.with(|ctx| {
            while sent < num {
                ctx.service.enqueue_wr_with(|ptr, count| unsafe {
                    let to_send = (num - sent).min(count);
                    for i in 0..to_send {
                        let wr = dp::WorkRequest::Reply(msg_buffer[sent + i]);
                        ptr.add(i).cast::<dp::WorkRequest>().write(wr);
                    }
                    sent += to_send;
                    to_send
                })?;
            }
            Result::<(), Error>::Ok(())
        })?;

        msg_buffer.clear();
        Ok(())
    }

    fn dispatch_requests<'s>(
        &'s self,
        running: &mut FuturesUnordered<LocalFutureObj<'s, MessageErased>>,
    ) -> Result<(), Error> {
        match check_completion_queue() {
            Ok(_) => {}
            Err(Error::Disconnect(conn_id)) => {
                // close the connection and free the related resources
                self.close_connection(conn_id)?;
            }
            Err(e) => return Err(e.into()),
        }

        for request in RECV_REQUEST_CACHE.get_mut(&self.stub_id).unwrap().drain(..) {
            let service_id = request.meta.service_id;
            match self.routes.get(&service_id) {
                Some(s) => {
                    match self.connections.borrow().get(&request.meta.conn_id) {
                        Some(conn) => {
                            let read_heap = Arc::clone(&conn.read_heap);
                            // Box allocation here!
                            // let task = LocalFutureObj::new(Box::new(s.call(request, read_heap)));
                            let task = LocalFutureObj::new(s.call(request, read_heap));
                            running.push(task);
                        }
                        None => {
                            // the connection has disappeared, do nothing
                        }
                    }
                }
                None => {
                    log::warn!("unrecognized request: {:?}", request);
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
        CONN_SERVER_STUB_MAP.with_borrow_mut(|map| {
            for conn_id in self.connections.borrow().keys() {
                map.remove(conn_id);
            }
        });

        RECV_REQUEST_CACHE.remove(&self.stub_id);

        // remove all outstanding RPC releated to this client/server stub
        PENDING_WREF.erase_by(|rpc_id, _| self.connections.borrow().contains_key(&rpc_id.0));
    }
}

pub trait NamedService {
    const SERVICE_ID: u32;
    const NAME: &'static str = "";
}

#[crate::async_trait]
pub trait Service {
    // return erased reply and ID of its corresponding RpcMessage
    async fn call(&self, req: MessageErased, read_heap: Arc<ReadHeap>) -> MessageErased;
}

pub fn service_pre_handler<'a, T: Unpin>(
    req: &MessageErased,
    read_heap: &'a Arc<ReadHeap>,
) -> RRef<'a, T> {
    RRef::new(&req, &read_heap)
}

pub fn service_post_handler<T: RpcData>(
    reply: WRef<T>,
    req_opaque: &MessageErased,
) -> MessageErased {
    // construct meta
    let meta = MessageMeta {
        msg_type: RpcMsgType::Response,
        ..req_opaque.meta
    };

    // track the msg as pending
    PENDING_WREF.insert(RpcId::new(meta.conn_id, meta.func_id), WRef::clone(&reply));

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
