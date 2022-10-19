// use std::cell::RefCell;
use std::future::Future;
use std::marker::PhantomData;
use std::net::ToSocketAddrs;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use interface::rpc::{CallId, MessageErased, MessageMeta, RpcId, RpcMsgType, TransportStatus};
use interface::AsHandle;
use ipc::channel::{Receiver, TryRecvError};
use ipc::mrpc::cmd::{Command, CompletionKind};
use ipc::mrpc::dp;
use libkoala::_rx_recv_impl as rx_recv_impl;

use super::conn::Connection;
use super::reply_cache::ReplyCache;
use super::RpcData;
use super::LOCAL_REACTOR;
use crate::{Error, RRef, ReadHeap, Status, WRef, MRPC_CTX};

#[cfg(feature = "timing")]
use crate::timing::{SampleKind, Timer};

#[cfg(feature = "timing")]
thread_local! {
    static TIMER: std::cell::RefCell<Timer> = std::cell::RefCell::new(Timer::new());
}

pub struct ReqFuture<'a, T> {
    rpc_id: RpcId,
    client: &'a ClientStub,
    _marker: PhantomData<T>,
}

impl<'a, T: Unpin> Future for ReqFuture<'a, T> {
    type Output = Result<RRef<T>, Status>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        futures::ready!(LOCAL_REACTOR.with_borrow_mut(|r| r.poll(cx)))?;

        this.client.dispatch()?;
        // let inner = this.client.inner.borrow();
        let inner = this.client.inner.lock();

        // Poll::Pending
        if let Some(reply) = inner
            .reply_cache
            .get(this.rpc_id.1)
            .expect("Expect an entry")
        {
            let ret = match reply {
                Ok(reply) => {
                    tracing::trace!(
                        "ReqFuture receive reply from mRPC engine, rpc_id={:?}",
                        this.rpc_id
                    );
                    let read_heap = this
                        .client
                        .conn
                        .map_alive(|alive| Arc::clone(&alive.read_heap))
                        .expect("TODO: return an error when connection is dead rather than panic");
                    Ok(RRef::new(&reply, read_heap))
                }
                Err(status) => Err(Status::from_incoming_transport(*status)),
            };
            return Poll::Ready(ret);
        }

        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl !Send for ClientStub {}
impl !Sync for ClientStub {}

#[derive(Debug)]
pub struct ClientStub {
    // A connection could go into error state, in that case, all subsequent operations over this
    // connection would return an error.
    conn: Connection,
    // inner: RefCell<Inner>,
    inner: spin::Mutex<Inner>,
}

#[derive(Debug)]
struct Inner {
    // Receiver
    receiver: Receiver<dp::Completion>,
    // Reply cache records whether a reply has been received for RPC client.  Each reply cache
    // should be assoicated to a connection.
    reply_cache: ReplyCache,
}

impl ClientStub {
    pub fn unary<Req, Res>(
        &self,
        service_id: u32,
        func_id: u32,
        call_id: CallId,
        req: WRef<Req>,
    ) -> impl Future<Output = Result<RRef<Res>, Status>> + '_
    where
        Req: RpcData,
        Res: Unpin + RpcData,
    {
        let conn_id = self.conn.handle();

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
            client: self,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn initiate_call(&self) -> CallId {
        // self.inner.borrow_mut().reply_cache.initiate_call()
        self.inner.lock().reply_cache.initiate_call()
    }
}

impl ClientStub {
    /// Dispatch one completion from the Receiver, and update PendingWRef and ReplyCache.
    fn dispatch_one(&self, comp: &dp::Completion, inner: &mut Inner) -> Result<(), Error> {
        match comp {
            &dp::Completion::Incoming(msg) => {
                let call_id = msg.meta.call_id;
                match msg.meta.msg_type {
                    RpcMsgType::Request => {
                        // server receives requests
                        panic!("impossible, something is wrong")
                    }
                    RpcMsgType::Response => {
                        // client receives responses, update the ReplyCache
                        inner.reply_cache.update(call_id, Ok(msg)).unwrap();
                    }
                }
            }
            &dp::Completion::Outgoing(rpc_id, status) => {
                // Receive an Ack for an previous outgoing RPC.
                self.conn.map_alive(|alive| alive.pending.remove(&rpc_id))?;

                if let TransportStatus::Error(_) = status {
                    // Update the ReplyCache with error
                    inner.reply_cache.update(rpc_id.1, Err(status)).unwrap();
                }
            }
            &dp::Completion::RecvError(conn_id, status) => {
                // On recv error, the peer probably disconnects.
                // The client should release the relevant resources of this connection.
                // Just drop the connection structure should be fine.
                log::debug!(
                    "Connection {:?} disconnected, status: {:?}",
                    conn_id,
                    status
                );
                self.conn.close();
            }
        }

        Ok(())
    }

    /// Dispatch completions from the Receiver.
    pub(crate) fn dispatch(&self) -> Result<(), Error> {
        // Because for client, each stub only has one connection, there is no real dispatch here.
        // let mut inner = self.inner.borrow_mut();
        let mut inner = self.inner.lock();

        loop {
            let item = inner.receiver.try_recv();
            match item {
                Ok(comp) => self.dispatch_one(&comp, &mut inner)?,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    panic!("Please ensure Reactor is running.");
                }
            }
        }

        Ok(())
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
        // self.conn
        //     .hold_rpc(RpcId::new(meta.conn_id, meta.call_id), WRef::clone(&msg))?;

        self.conn.map_alive(|alive| {
            alive
                .pending
                .insert(RpcId::new(meta.conn_id, meta.call_id), WRef::clone(&msg))
        })?;

        // construct the request
        let (ptr_app, ptr_backend) = msg.into_shmptr().to_raw_parts();
        let erased = MessageErased {
            meta,
            shm_addr_app: ptr_app.addr().get(),
            shm_addr_backend: ptr_backend.addr().get(),
        };

        let req = dp::WorkRequest::Call(erased);

        #[cfg(feature = "timing")]
        TIMER.with_borrow_mut(|timer| {
            timer.sample(
                RpcId::new(meta.conn_id, meta.call_id),
                SampleKind::ClientRequest,
            );
        });

        // notify the backend
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

                // register the stub with the reactor
                let conn = Connection::new(conn_handle, read_heap);
                let (stub_id, receiver) = LOCAL_REACTOR.with_borrow_mut(|r| r.register_stub());
                LOCAL_REACTOR.with_borrow_mut(|r| r.register_connection(stub_id, &conn));

                Ok(Self {
                    conn,
                    // inner: RefCell::new(Inner {
                    inner: spin::Mutex::new(Inner {
                        receiver,
                        reply_cache: ReplyCache::new(),
                    }),
                })
            })
        })
    }
}
