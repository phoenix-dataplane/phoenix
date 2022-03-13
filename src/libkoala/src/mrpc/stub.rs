use std::collections::HashSet;
use std::io;
use std::net::ToSocketAddrs;

use unique::Unique;

use ipc::mrpc::cmd::{Command, CompletionKind};
use ipc::mrpc::dp;

/// Re-exports
pub use interface::rpc::{MessageMeta, MessageTemplateErased, RpcMsgType};
use interface::Handle;
pub use ipc::mrpc::control_plane::TransportType;

use crate::mrpc::codegen::SwitchAddressSpace;
use crate::mrpc::shared_heap::SharedHeapAllocator;
use crate::mrpc::{Error, MRPC_CTX};
use crate::rx_recv_impl;
use crate::verbs::MemoryRegion;

#[derive(Debug)]
pub struct MessageTemplate<T> {
    meta: MessageMeta,
    val: Unique<T>,
}

impl<T> MessageTemplate<T> {
    pub fn new(mut val: T, conn_id: Handle) -> Self {
        let meta = MessageMeta {
            conn_id,
            func_id: 0,
            call_id: 0,
            len: 0,
            msg_type: RpcMsgType::Request,
        };
        Self {
            meta,
            val: Unique::new(&mut val as *mut T).unwrap(),
        }
    }
}

unsafe impl<T: SwitchAddressSpace> SwitchAddressSpace for MessageTemplate<T> {
    fn switch_address_space(&mut self) {
        unsafe {
            self.val.as_mut().switch_address_space();
            self.val = Unique::new(
                self.val
                    .as_ptr()
                    .offset(SharedHeapAllocator::query_shm_offset(self.val.as_ptr() as _)),
            )
            .unwrap();
        }
    }
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
                let reply_mrs = ret
                    .1
                    .into_iter()
                    .zip(&fds)
                    .map(|(mr, &fd)| {
                        let memfd = Memfd::try_from_fd(fd)
                            .map_err(|_| io::Error::last_os_error())
                            .unwrap();
                        MemoryRegion::new(mr.pd, mr.handle, mr.rkey, mr.vaddr, memfd).unwrap()
                    })
                    .collect();
                Ok(Self {
                    handle: ret.0,
                    reply_mrs,
                })
            })
        })
    }

    pub fn post<T: SwitchAddressSpace>(&self, mut msg: MessageTemplate<T>) -> Result<(), Error> {
        msg.switch_address_space();
        let erased = MessageTemplateErased {
            meta: msg.meta,
            shmptr: msg.val.as_ptr() as u64,
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
}

#[derive(Debug)]
pub struct ServerStub {
    listener_handle: Handle,
    handles: HashSet<Handle>,
    mrs: HashSet<MemoryRegion<u8>>,
}

impl ServerStub {
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
                })
            })
        })
    }
}
