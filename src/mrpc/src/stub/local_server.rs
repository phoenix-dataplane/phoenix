use std::cell::RefCell;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::task::Poll;
use std::future::Future;

use fnv::FnvHashMap as HashMap;
use futures::future::poll_fn;
use futures::select;
use futures::stream::{FuturesUnordered, StreamExt};
use futures::task::LocalFutureObj;
use futures::FutureExt;

use interface::rpc::{MessageErased, RpcId, RpcMsgType, TransportStatus};
use interface::{AsHandle, Handle};
use ipc::channel::{Receiver, TryRecvError};
use ipc::mrpc::cmd::{Command, CompletionKind, ConnectResponse};
use ipc::mrpc::dp;
use libkoala::_rx_recv_impl as rx_recv_impl;

use super::conn::Connection;
use super::service::{NamedService, Service};
use super::LOCAL_REACTOR;
use crate::wref::WRefOpaque;
use crate::{Error, ReadHeap, MRPC_CTX};

#[cfg(feature = "timing")]
use crate::timing::{SampleKind, Timer};

#[cfg(feature = "timing")]
thread_local! {
    static TIMER: std::cell::RefCell<Timer> = std::cell::RefCell::new(Timer::new());
}

pub struct LocalServer {
    stub_id: usize,
    listener_handle: Handle,
    routes: HashMap<u32, Box<dyn Service>>,
    inner: RefCell<Inner>,
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        eprintln!("do something with listener_handle {:?}", self.listener_handle);
    }
}

impl !Send for LocalServer {}
impl !Sync for LocalServer {}

pub(crate) struct Inner {
    // Receiver.
    receiver: Receiver<dp::Completion>,
    // Connections.
    connections: HashMap<Handle, Connection>,
}

impl Inner {
    #[inline]
    fn get_connection(&self, conn_id: Handle) -> Result<&Connection, Error> {
        self.connections
            .get(&conn_id)
            .ok_or(Error::ConnectionClosed)
    }

    fn close_connection(&mut self, conn_id: Handle) {
        self.connections.remove(&conn_id);
    }
}

impl LocalServer {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> Result<Self, Error> {
        let bind_addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or(Error::NoAddrResolved)?;
        let req = Command::Bind(bind_addr);
        MRPC_CTX.with(|ctx| {
            ctx.service.send_cmd(req)?;
            rx_recv_impl!(ctx.service, CompletionKind::Bind, listener_handle, {
                let (stub_id, receiver) = LOCAL_REACTOR.with_borrow_mut(|r| r.register_stub());

                Ok(Self {
                    stub_id,
                    listener_handle,
                    routes: HashMap::default(),
                    inner: RefCell::new(Inner {
                        connections: HashMap::default(),
                        receiver,
                    }),
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
        // running tasks
        let mut running = FuturesUnordered::new();
        running.push(LocalFutureObj::new(Box::new(std::future::pending())));

        // batching reply small requests for better CPU efficiency
        let mut reply_buffer = Vec::with_capacity(32);

        poll_fn(|cx| {
            loop {
                select! {
                    reply_erased = running.next() => {
                        if reply_erased.is_none() { continue; }
                        reply_buffer.push(reply_erased.unwrap());
                    }
                    // has_queue_event = poll_fn(|cx| LOCAL_REACTOR.with_borrow_mut(|r| r.poll(cx))) => {
                    //     self.dispatch_requests(&mut running)?;
                    // }
                    complete => {
                        panic!("unexpected complete")
                    }
                    default => {
                        // TODO(cjr): Having the default branch is not cpu efficient
                        if !reply_buffer.is_empty() {
                            self.post_replies(&mut reply_buffer)?;
                        }
                        // no futures is ready
                        self.check_cm_event()?;
                        // check new requests, dispatch them to the executor
                        match LOCAL_REACTOR.with_borrow_mut(|r| r.poll(cx)) {
                            Poll::Ready(Ok(n)) if n > 0 => self.dispatch_requests(&mut running)?,
                            _ => break Poll::Pending,
                        }
                    }
                } // end select
            } // end loop
        })
        .await
    }

    pub async fn serve_with_graceful_shutdown<F>(&mut self, shutdown: F) -> Result<(), Error>
    where
        F: Future<Output = ()> + Unpin,
    {
        let mut shutdown = shutdown.fuse();

        // running tasks
        let mut running = FuturesUnordered::new();
        running.push(LocalFutureObj::new(Box::new(std::future::pending())));

        // batching reply small requests for better CPU efficiency
        let mut reply_buffer = Vec::with_capacity(32);

        poll_fn(|cx| {
            loop {
                select! {
                    reply_erased = running.next() => {
                        if reply_erased.is_none() { continue; }
                        reply_buffer.push(reply_erased.unwrap());
                    }
                    // has_queue_event = poll_fn(|cx| LOCAL_REACTOR.with_borrow_mut(|r| r.poll(cx))) => {
                    //     self.dispatch_requests(&mut running)?;
                    // }
                    _ = shutdown => {
                        break Poll::Ready(Ok(()));
                    },
                    complete => {
                        panic!("unexpected complete")
                    }
                    default => {
                        // TODO(cjr): Having the default branch is not cpu efficient
                        if !reply_buffer.is_empty() {
                            self.post_replies(&mut reply_buffer)?;
                        }
                        // no futures is ready
                        self.check_cm_event()?;
                        // check new requests, dispatch them to the executor
                        match LOCAL_REACTOR.with_borrow_mut(|r| r.poll(cx)) {
                            Poll::Ready(Ok(n)) if n > 0 => self.dispatch_requests(&mut running)?,
                            _ => break Poll::Pending,
                        }
                    }
                } // end select
                todo!("yield");
            } // end loop
        })
        .await
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

                let read_heap = ReadHeap::new(&conn_resp, &fds);
                let vaddrs = read_heap
                    .rbufs
                    .iter()
                    .map(|rbuf| (rbuf.as_handle(), rbuf.as_ptr().expose_addr()))
                    .collect();

                // register connection to the reactor
                let conn = Connection::new(conn_handle, read_heap);
                LOCAL_REACTOR.with_borrow_mut(|r| r.register_connection(self.stub_id, &conn));

                // update connection set
                assert_eq!(
                    self.inner
                        .borrow_mut()
                        .connections
                        .insert(conn_handle, conn),
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

    fn post_replies(&self, msg_buffer: &mut Vec<(WRefOpaque, MessageErased)>) -> Result<(), Error> {
        // track the msg as pending

        for m in msg_buffer.iter() {
            let conn_id = m.1.meta.conn_id;
            let inner = self.inner.borrow();
            let conn = inner.get_connection(conn_id)?;
            conn.map_alive(|alive| {
                alive
                    .pending
                    .insert_opaque(RpcId::new(conn.handle(), m.1.meta.call_id), m.0.clone())
            })?;
        }

        let num = msg_buffer.len();
        let mut sent = 0;
        MRPC_CTX.with(|ctx| {
            while sent < num {
                ctx.service.enqueue_wr_with(|ptr, count| unsafe {
                    let to_send = (num - sent).min(count);
                    for i in 0..to_send {
                        let wr = dp::WorkRequest::Reply(msg_buffer[sent + i].1);
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

    fn dispatch_one_request<'s>(
        &'s self,
        comp: &dp::Completion,
        inner: &mut Inner,
        running: &mut FuturesUnordered<LocalFutureObj<'s, (WRefOpaque, MessageErased)>>,
    ) -> Result<(), Error> {
        match comp {
            &dp::Completion::Incoming(request) => {
                match request.meta.msg_type {
                    RpcMsgType::Request => {
                        // server receives requests
                        // todo!("do something with the request");
                        let service_id = request.meta.service_id;
                        match self.routes.get(&service_id) {
                            Some(s) => {
                                let conn = inner.get_connection(request.meta.conn_id)?;
                                // the connection has disappeared, do nothing

                                let read_heap =
                                    conn.map_alive(|alive| Arc::clone(&alive.read_heap))?;
                                let task = LocalFutureObj::new(s.call(request, read_heap));
                                running.push(task);
                            }
                            None => {
                                log::warn!("unrecognized request: {:?}", request);
                            }
                        }
                    }
                    RpcMsgType::Response => {
                        // client receives responses, update the ReplyCache
                        panic!("impossible, something is wrong")
                    }
                }
            }
            &dp::Completion::Outgoing(rpc_id, status) => {
                // Receive an Ack for a previous outgoing RPC.
                inner
                    .get_connection(rpc_id.0)?
                    .map_alive(|alive| alive.pending.remove(&rpc_id))?;

                if let TransportStatus::Error(_) = status {
                    log::warn!(
                        "Error during sending reply for {:?}, status: {:?}",
                        rpc_id,
                        status
                    );
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
                inner.close_connection(conn_id);
            }
        }

        Ok(())
    }

    fn dispatch_requests<'s>(
        &'s self,
        running: &mut FuturesUnordered<LocalFutureObj<'s, (WRefOpaque, MessageErased)>>,
    ) -> Result<(), Error> {
        let mut inner = self.inner.borrow_mut();
        loop {
            let item = inner.receiver.try_recv();
            match item {
                Ok(ref comp) => {
                    self.dispatch_one_request(comp, &mut inner, running)?;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    panic!("Please ensure Reactor is running.");
                }
            }
        }

        Ok(())
    }
}
