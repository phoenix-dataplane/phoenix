use std::net::ToSocketAddrs;

use ipc::mrpc::cmd::{Command, CompletionKind};

/// Re-exports
pub use interface::rpc::{
    Marshal, RpcMessage, RpcMsgType, SgList, ShmBuf, SwitchAddressSpace, Unmarshal,
};
pub use ipc::mrpc::control_plane::TransportType;

use interface::Handle;

use crate::mrpc::MRPC_CTX;
use crate::{rx_recv_impl, Error};

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
}
