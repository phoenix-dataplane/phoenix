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
use crate::salloc::gc::MESSAGE_ID_COUNTER;
use crate::salloc::owner::{AllocOwner, AppOwned};
use crate::salloc::region::SharedRegion;
use crate::salloc::SA_CTX;


pub(crate) mod ownership {
    pub trait AppOwendRequest {}
    pub trait AppOwendReply {}
}

// NOTE(wyj): RpcMessage is used to send messages to backend. 
// T must be app-owned.
#[derive(Debug)]
pub struct RpcMessage<T> {
    pub(crate) inner: ManuallyDrop<Box<MessageTemplate<T, AppOwned>, AppOwned>>,
    // ID of this RpcMessage
    pub(crate) identifier: u64,
    // How many times the message is sent
    pub(crate) send_count: u64
}

impl<T: ownership::AppOwendRequest> RpcMessage<T> {
    pub fn new_request(msg: T) -> Self {
        let msg = Box::new(msg);
        let inner = MessageTemplate::new_request(msg, Handle(0), 0, 0);
        let message_id = MESSAGE_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        RpcMessage { 
            inner: ManuallyDrop::new(inner),
            identifier: message_id,
            send_count: 0 
        }
    }
}

impl<T: ownership::AppOwendReply> RpcMessage<T> {
    pub fn new_reply(msg: T) -> Self {
        let msg = Box::new(msg);
        let inner = MessageTemplate::new_reply(msg, Handle(0), 0, 0);
        let message_id = MESSAGE_ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        RpcMessage { 
            inner: ManuallyDrop::new(inner),
            identifier: message_id,
            send_count: 0
        }
    }
}

impl<T> Deref for RpcMessage<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { self.inner.as_ref().val.as_ref() }
    }
}

impl<T> Drop for RpcMessage<T> {
    
}

unsafe impl<T: SwitchAddressSpace> SwitchAddressSpace for RpcMessage<T> {
    fn switch_address_space(&mut self) {
        self.inner.switch_address_space();    
    }
}


#[derive(Debug)]
pub struct MessageTemplate<T, O: AllocOwner> {
    pub(crate) meta: MessageMeta,
    pub(crate) val: ShmPtr<T>,
    _owner: O
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
            _owner: AppOwned
        })
    }

    pub(crate) fn new_reply(
        val: Box<T>, 
        conn_id: Handle,
        func_id: u32,
        call_id: u32
    ) -> Box<Self> {
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
            _owner: AppOwned
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
    // TODO
    fn switch_address_space(&mut self) {
        unsafe { self.val.as_mut() }.switch_address_space();
    }
}

pub(crate) fn post_request<T: SwitchAddressSpace>(
    msg: &mut Box<MessageTemplate<T, AppOwned>>,
) -> Result<(), Error> {
    let meta = msg.meta;
    tracing::trace!(
        "client post request to mRPC engine, call_id={}",
        meta.call_id
    );

    msg.switch_address_space();
    let (ptr, addr_remote) = Box::to_raw_parts(&msg);
    let erased = MessageTemplateErased {
        meta,
        shm_addr: addr_remote.addr().get(),
        shm_addr_remote: ptr.addr().get(),
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

pub(crate) fn post_reply(erased: MessageTemplateErased) -> Result<(), Error> {
    tracing::trace!(
        "client post reply to mRPC engine, call_id={}",
        erased.meta.call_id
    );

    let req = dp::WorkRequest::Reply(erased);
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

#[derive(Debug)]
pub struct ClientStub {
    // mRPC connection handle
    handle: Handle,
    reply_mrs: Vec<SharedRegion>,
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
                        let m = SharedRegion::new(
                            mr.1,
                            mr.2,
                            // TODO(cjr): update this
                            8 * 1024 * 1024,
                            mr.3,
                            memfd,
                        )
                        .unwrap();
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
                Ok(Self {
                    handle: ret.0,
                    reply_mrs,
                })
            })
        })
    }
}

pub struct Server {
    listener_handle: Handle,
    handles: HashSet<Handle>,
    mrs: HashMap<Handle, SharedRegion>,
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
                Ok(Self {
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
                    rx_recv_impl!(ctx.service, CompletionKind::NewConnection, ret, {
                        use memfd::Memfd;
                        assert_eq!(fds.len(), ret.1.len());
                        assert!(self.handles.insert(ret.0));
                        for (mr, &fd) in ret.1.into_iter().zip(&fds) {
                            let memfd = Memfd::try_from_fd(fd)
                                .map_err(|_| io::Error::last_os_error())
                                .unwrap();
                            let h = mr.0;
                            let m = SharedRegion::new(
                                mr.1,
                                mr.2,
                                // TODO(cjr): update this
                                8 * 1024 * 1024,
                                mr.3,
                                memfd,
                            )
                            .unwrap();
                            vaddrs.push((h, m.as_ptr().expose_addr()));
                            self.mrs.insert(h, m);
                        }
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

    fn post_replies(
        &mut self,
        msgs: Vec<MessageTemplateErased>,
    ) -> Result<(), std::boxed::Box<dyn std::error::Error>> {
        for m in msgs {
            post_reply(m)?;
        }
        Ok(())
    }

    fn poll_requests(
        &mut self,
    ) -> Result<Vec<MessageTemplateErased>, std::boxed::Box<dyn std::error::Error>> {
        let mut msgs = Vec::with_capacity(32);
        MRPC_CTX.with(|ctx| {
            ctx.service.dequeue_wc_with(|ptr, count| unsafe {
                // just do it in place, no threadpool
                for i in 0..count {
                    let c = ptr.add(i).cast::<dp::Completion>().read();
                    // looking up the routing table
                    let func_id = c.erased.meta.func_id;
                    match self.routes.get_mut(&func_id) {
                        Some(s) => {
                            let reply_erased = s.call(c.erased);
                            msgs.push(reply_erased);
                        }
                        None => {
                            eprintln!("unrecognized request: {:?}", c.erased);
                        }
                    }
                }
                count
            })?;
            Ok(msgs)
        })
    }
}

pub trait NamedService {
    const FUNC_ID: u32;
}

pub trait Service {
    fn call(&mut self, req: MessageTemplateErased) -> MessageTemplateErased;
}
