use std::collections::{HashMap, HashSet};
use std::io;
use std::net::ToSocketAddrs;

use unique::Unique;

use ipc::mrpc::cmd::{Command, CompletionKind};
use ipc::mrpc::dp;

/// Re-exports
pub use interface::rpc::{MessageMeta, MessageTemplateErased, RpcMsgType};
use interface::Handle;
pub use ipc::mrpc::control_plane::TransportType;

use crate::mrpc::alloc::Box;
use crate::mrpc::codegen::SwitchAddressSpace;
use crate::salloc::heap::SharedHeapAllocator;
use crate::mrpc::{Error, MRPC_CTX};
use crate::rx_recv_impl;
use crate::verbs::MemoryRegion;

#[derive(Debug)]
pub struct MessageTemplate<T> {
    pub(crate) meta: MessageMeta,
    pub(crate) val: Unique<T>,
}

impl<T> MessageTemplate<T> {
    pub fn new(val: Box<T>, conn_id: Handle, func_id: u32, call_id: u64) -> Box<Self> {
        // TODO(cjr): fill in these values
        let meta = MessageMeta {
            conn_id,
            func_id,
            call_id,
            len: 0,
            msg_type: RpcMsgType::Request,
        };
        Box::new(
            Self {
                meta,
                val: Unique::new(Box::into_raw(val)).unwrap(),
            }
        )
    }

    pub fn new_reply(val: Box<T>, conn_id: Handle, func_id: u32, call_id: u64) -> Box<Self> {
        let meta = MessageMeta {
            conn_id,
            func_id,
            call_id,
            len: 0,
            msg_type: RpcMsgType::Response,
        };
        Box::new(
            Self {
                meta,
                val: Unique::new(Box::into_raw(val)).unwrap(),
            },
        )
    }
}

unsafe impl<T: SwitchAddressSpace> SwitchAddressSpace for MessageTemplate<T> {
    fn switch_address_space(&mut self) {
        unsafe { self.val.as_mut() }.switch_address_space();
        self.val = Unique::new(
            self.val
                .as_ptr()
                .cast::<u8>()
                .wrapping_offset(SharedHeapAllocator::query_shm_offset(self.val.as_ptr() as _))
                .cast(),
        )
        .unwrap();
    }
}

pub(crate) fn post_request<T: SwitchAddressSpace>(
    mut msg: Box<MessageTemplate<T>>,
) -> Result<(), Error> {
    let meta = msg.meta;
    msg.switch_address_space();
    let local_ptr = Box::into_raw(msg);
    let remote_shmptr = local_ptr
        .cast::<u8>()
        .wrapping_offset(SharedHeapAllocator::query_shm_offset(local_ptr as _))
        as u64;
    let erased = MessageTemplateErased {
        meta,
        shm_addr: remote_shmptr,
        shm_addr_remote: local_ptr as *const() as u64
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
    reply_mrs: Vec<MemoryRegion<u8>>,
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
                        let m = MemoryRegion::new(
                            mr.pd,
                            mr.handle,
                            mr.rkey,
                            mr.vaddr,
                            mr.map_len as usize,
                            mr.file_off,
                            memfd,
                        )
                        .unwrap();
                        vaddrs.push((mr.handle.0, m.as_ptr() as u64));
                        m
                    })
                    .collect();
                // return the mapped addr back
                let req = Command::NewMappedAddrs(vaddrs);
                ctx.service.send_cmd(req)?;
                // COMMENT(cjr): must wait for the reply!
                rx_recv_impl!(ctx.service, CompletionKind::NewMappedAddrs)?;
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
    mrs: HashMap<Handle, MemoryRegion<u8>>,
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
                            let h = mr.handle.0;
                            let m = MemoryRegion::new(
                                mr.pd,
                                mr.handle,
                                mr.rkey,
                                mr.vaddr,
                                mr.map_len as usize,
                                mr.file_off,
                                memfd,
                            )
                            .unwrap();
                            vaddrs.push((h, m.as_ptr() as u64));
                            self.mrs.insert(h, m);
                        }
                        Ok(())
                    })?;
                    // return the mapped addr back
                    let req = Command::NewMappedAddrs(vaddrs);
                    ctx.service.send_cmd(req)?;
                    // COMMENT(cjr): must wait for the reply!
                    rx_recv_impl!(ctx.service, CompletionKind::NewMappedAddrs)?;
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
