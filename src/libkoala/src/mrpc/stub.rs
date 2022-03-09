use std::net::ToSocketAddrs;

use unique::Unique;

use ipc::mrpc::cmd::{Command, CompletionKind};
use ipc::mrpc::dp;

/// Re-exports
pub use interface::rpc::{MessageMeta, MessageTemplateErased, RpcMsgType};
pub use ipc::mrpc::control_plane::TransportType;

use interface::Handle;

use crate::mrpc::codegen::SwitchAddressSpace;
use crate::mrpc::shared_heap::SharedHeapAllocator;
use crate::mrpc::{MRPC_CTX, Error};
use crate::rx_recv_impl;

#[derive(Debug)]
pub struct MessageTemplate<T> {
    meta: MessageMeta,
    val: Unique<T>,
}

impl<T> MessageTemplate<T> {
    pub fn new(mut val: T) -> Self {
        let meta = MessageMeta {
            conn_id: 0,
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

// impl<T: Unmarshal> Unmarshal for MessageTemplate<T> {
//     type Error = ();
//     fn unmarshal(mut sg_list: SgList) -> Result<Self, Self::Error> {
//         if sg_list.0.len() <= 1 {
//             return Err(());
//         }
//         let header_sgl = sg_list.0.remove(0);
//         let meta = MessageMeta::unmarshal(header_sgl)?;
//         let val = T::unmarshal(sg_list).or(Err(()))?;
//         Ok(Self {
//             meta,
//             val,
//         })
//     }
// }
//
// impl<T: Send + Marshal + Unmarshal> RpcMessage for MessageTemplate<T> {
//     #[inline]
//     fn conn_id(&self) -> u32 { self.meta.conn_id }
//     #[inline]
//     fn func_id(&self) -> u32 { self.meta.func_id }
//     #[inline]
//     fn call_id(&self) -> u64 { self.meta.call_id }
//     #[inline]
//     fn len(&self) -> u64 { self.meta.len }
//     #[inline]
//     fn is_request(&self) -> bool { self.meta.msg_type == RpcMsgType::Request }
//     fn marshal(&self) -> SgList {
//         self.marshal().unwrap()
//     }
// }

#[derive(Debug)]
pub struct ClientStub {
    // mRPC connection handle
    handle: Handle,
}

impl ClientStub {
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
            rx_recv_impl!(ctx.service, CompletionKind::Connect, handle, {
                Ok(Self { handle })
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
