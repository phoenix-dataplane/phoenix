use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io;
use std::mem::ManuallyDrop;
use std::net::ToSocketAddrs;
use std::ops::Deref;

use ipc::mrpc::cmd::{Command, CompletionKind};
use ipc::mrpc::dp;
use ipc::shmalloc::ShmPtr;

/// Re-exports
pub use interface::rpc::{MessageMeta, MessageTemplateErased, RpcMsgType};
use interface::Handle;
pub use ipc::mrpc::control_plane::TransportType;
use ipc::shmalloc::SwitchAddressSpace;

use crate::mrpc::alloc::Box;
use crate::mrpc::{Error, MRPC_CTX};
use crate::rx_recv_impl;
use crate::salloc::gc::{
    CS_OUTSTANDING_WR_CNT, CS_STUB_ID_COUNTER, GARBAGE_COLLECTOR, MESSAGE_ID_COUNTER,
    OUTSTANDING_WR,
};
use crate::salloc::owner::{AllocOwner, AppOwned};
use crate::salloc::region::SharedRecvBuffer;

use crate::salloc::SA_CTX;

thread_local! {
    pub(crate) static RECV_CACHE: RefCell<HashMap<(Handle, u32), MessageTemplateErased>> = RefCell::new(HashMap::new());
}

pub(crate) mod ownership {
    pub trait AppOwendRequest {}
    pub trait AppOwendReply {}
}

// TODO(wyj): pub(crate) visibility?
pub trait RpcData: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> RpcData for T {}

#[derive(Debug)]
pub struct RpcMessage<T: RpcData> {
    pub(crate) inner: ManuallyDrop<Box<MessageTemplate<T, AppOwned>, AppOwned>>,
    // ID of this RpcMessage
    pub(crate) identifier: u64,
    // How many times the message is sent
    pub(crate) send_count: u64,
}

impl<T: ownership::AppOwendRequest + RpcData> RpcMessage<T> {
    pub fn new_request(msg: T) -> Self {
        let msg = Box::new(msg);
        let inner = MessageTemplate::new_request(msg, Handle(0), 0, 0);
        let message_id = MESSAGE_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        RpcMessage {
            inner: ManuallyDrop::new(inner),
            identifier: message_id,
            send_count: 0,
        }
    }
}

impl<T: ownership::AppOwendReply + RpcData> RpcMessage<T> {
    pub fn new_reply(msg: T) -> Self {
        let msg = Box::new(msg);
        let inner = MessageTemplate::new_reply(msg, Handle(0), 0, 0);
        let message_id = MESSAGE_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        RpcMessage {
            inner: ManuallyDrop::new(inner),
            identifier: message_id,
            send_count: 0,
        }
    }
}

impl<T: RpcData> Deref for RpcMessage<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { self.inner.as_ref().val.as_ref() }
    }
}

impl<T: RpcData> Drop for RpcMessage<T> {
    fn drop(&mut self) {
        let inner = unsafe { ManuallyDrop::take(&mut self.inner) };
        GARBAGE_COLLECTOR.collect(inner, self.identifier, self.send_count)
    }
}

unsafe impl<T: SwitchAddressSpace + RpcData> SwitchAddressSpace for RpcMessage<T> {
    fn switch_address_space(&mut self) {
        self.inner.switch_address_space();
    }
}

#[derive(Debug)]
pub struct MessageTemplate<T, O: AllocOwner> {
    pub(crate) meta: MessageMeta,
    pub(crate) val: ShmPtr<T>,
    _owner: O,
}

impl<T> MessageTemplate<T, AppOwned> {
    pub(crate) fn new_request(
        val: Box<T>,
        conn_id: Handle,
        func_id: u32,
        call_id: u32,
    ) -> Box<Self> {
        // TODO(cjr): fill in these values
        let meta = MessageMeta {
            conn_id,
            func_id,
            call_id,
            len: 0,
            msg_type: RpcMsgType::Request,
        };
        Box::new(Self {
            meta,
            val: Box::into_shmptr(val),
            _owner: AppOwned,
        })
    }

    pub(crate) fn new_reply(val: Box<T>, conn_id: Handle, func_id: u32, call_id: u32) -> Box<Self> {
        let meta = MessageMeta {
            conn_id,
            func_id,
            call_id,
            len: 0,
            msg_type: RpcMsgType::Response,
        };
        Box::new(Self {
            meta,
            val: Box::into_shmptr(val),
            _owner: AppOwned,
        })
    }
}

impl<T, O: AllocOwner> Drop for MessageTemplate<T, O> {
    fn drop(&mut self) {
        if O::is_app_owend() {
            let owned = unsafe { Box::from_shmptr(self.val) };
            std::mem::drop(owned);
        }
    }
}

unsafe impl<T: SwitchAddressSpace> SwitchAddressSpace for MessageTemplate<T, AppOwned> {
    fn switch_address_space(&mut self) {
        unsafe { self.val.as_mut() }.switch_address_space();
    }
}

pub(crate) fn check_completion_queue() -> Result<(), super::Error> {
    use ipc::mrpc::dp::Completion;

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
                Completion::Recv(msg) => {
                    let conn_id = msg.meta.conn_id;
                    let call_id = msg.meta.call_id;
                    RECV_CACHE.with(|cache| {
                        cache.borrow_mut().insert((conn_id, call_id), msg);
                    })
                }
                Completion::SendCompletion(conn_id, call_id) => {
                    let (msg_id, cs_id) = OUTSTANDING_WR.with(|outstanding| {
                        outstanding
                            .borrow_mut()
                            .remove(&(conn_id, call_id))
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

    pub(crate) fn post_request<T: SwitchAddressSpace + ownership::AppOwendRequest + RpcData>(
        &self,
        // msg: &mut Box<MessageTemplate<T, AppOwned>>,
        msg: &mut RpcMessage<T>,
    ) -> Result<(), Error> {
        let meta = msg.inner.meta;
        tracing::trace!(
            "client post request to mRPC engine, call_id={}",
            meta.call_id
        );
        // TODO(wyj): get rid of SwitchAddrSpace
        msg.switch_address_space();

        let (ptr, addr_remote) = Box::to_raw_parts(&msg.inner);
        let erased = MessageTemplateErased {
            meta,
            shm_addr: addr_remote.addr().get(),
            shm_addr_remote: ptr.addr().get(),
        };
        let req = dp::WorkRequest::Call(erased);

        // codegen should increase send_count for RpcMessage
        OUTSTANDING_WR.with(|outstanding| {
            outstanding
                .borrow_mut()
                .insert((meta.conn_id, meta.call_id), (msg.identifier, self.stub_id));
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
                CS_OUTSTANDING_WR_CNT.with(|counting| {
                    counting.borrow_mut().insert(stub_id, 0);
                });
                Ok(Self {
                    stub_id,
                    handle: ret.0,
                    reply_mrs,
                })
            })
        })
    }
}

impl !Send for ClientStub {}
impl !Sync for ClientStub {}

impl Drop for ClientStub {
    fn drop(&mut self) {
        while CS_OUTSTANDING_WR_CNT.with(|cnt| {
            *cnt.borrow()
                .get(&self.stub_id)
                .expect("client/server stub not found")
                > 0
        }) {
            check_completion_queue();
        }

        CS_OUTSTANDING_WR_CNT.with(|cnt| {
            cnt.borrow_mut()
                .remove(&self.stub_id)
                .expect("client/server stub not found")
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
    // func_id -> Service
    routes: HashMap<u32, std::boxed::Box<dyn Service>>,
}

impl Server {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> Result<Self, Error> {
        let bind_addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or(Error::NoAddrResolved)?;
        let req = Command::Bind(bind_addr);
        MRPC_CTX.with(|ctx| {
            ctx.service.send_cmd(req)?;
            rx_recv_impl!(ctx.service, CompletionKind::Bind, listener_handle, {
                let stub_id = CS_STUB_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Ok(Self {
                    stub_id,
                    listener_handle,
                    handles: HashSet::default(),
                    mrs: Default::default(),
                    routes: HashMap::default(),
                })
            })
        })
    }

    pub fn add_service<S: Service + NamedService + 'static>(&mut self, svc: S) -> &mut Self {
        match self.routes.insert(S::FUNC_ID, std::boxed::Box::new(svc)) {
            Some(_) => panic!("A func_id can only have 1 handler, func_id: {}", S::FUNC_ID),
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

    pub(crate) fn post_reply(
        &self,
        erased: MessageTemplateErased,
        msg_id: u64,
    ) -> Result<(), Error> {
        tracing::trace!(
            "client post reply to mRPC engine, call_id={}",
            erased.meta.call_id
        );

        let req = dp::WorkRequest::Reply(erased);

        // codegen should increase send_count for RpcMessage
        let meta = erased.meta;
        OUTSTANDING_WR.with(|outstanding| {
            outstanding
                .borrow_mut()
                .insert((meta.conn_id, meta.call_id), (msg_id, self.stub_id));
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

    fn post_replies(
        &mut self,
        msgs: Vec<(MessageTemplateErased, u64)>,
    ) -> Result<(), std::boxed::Box<dyn std::error::Error>> {
        for (reply, msg_id) in msgs {
            self.post_reply(reply, msg_id)?;
        }
        Ok(())
    }

    fn poll_requests(
        &mut self,
    ) -> Result<Vec<(MessageTemplateErased, u64)>, std::boxed::Box<dyn std::error::Error>> {
        let mut msgs = Vec::with_capacity(32);

        check_completion_queue()?;
        RECV_CACHE.with(|cache| {
            let mut borrow = cache.borrow_mut();
            let requests = borrow.drain_filter(|(conn_id, _), _| self.handles.contains(conn_id));

            for (_, req) in requests {
                let func_id = req.meta.func_id;
                match self.routes.get_mut(&func_id) {
                    Some(s) => {
                        let (reply_erased, msg_id) = s.call(req);
                        msgs.push((reply_erased, msg_id));
                    }
                    None => {
                        eprintln!("unrecognized request: {:?}", req);
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
        // remove all outstanding WR releated to this client/server stub
        OUTSTANDING_WR.with(|outstanding| {
            let borrow = outstanding.borrow_mut();
            let wrs = borrow.drain_filter(|_, (_msg_id, cs_id)| {
                *cs_id == self.stub_id
            });

            let msg_cnt = wrs.fold(HashMap::new(), |mut acc, (_, (msg_id, _))| {
                *acc.entry(msg_id).or_insert(0u64) += 1;
                acc
            });

            for (msg_id, cnt) in msg_cnt {

            }
        })
    }
}

pub trait NamedService {
    const FUNC_ID: u32;
}

pub trait Service {
    // return erased reply and ID of its corresponding RpcMessage
    fn call(&mut self, req: MessageTemplateErased) -> (MessageTemplateErased, u64);
}
