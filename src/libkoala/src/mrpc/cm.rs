use std::net::ToSocketAddrs;

use ipc::mrpc::cmd::{Command, CompletionKind};
use ipc::mrpc::control_plane::TransportType;
use ipc::service::Service;

use crate::mrpc::MRPC_CTX;
use crate::{Error, rx_recv_impl};

pub struct MrpcStub {}

impl MrpcStub {
    pub fn set_transport(transport_type: TransportType) -> Result<(), Error> {
        let req = Command::SetTransport(transport_type);
        MRPC_CTX.with(|ctx| {
            ctx.service.send_cmd(req)?;
            rx_recv_impl!(ctx.service, CompletionKind::SetTransport)?;
            Ok(())
        })
    }

    pub fn connect<A: ToSocketAddrs>(&self, addr: A) -> Result<(), Error> {
        let connect_addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or(Error::NoAddrResolved)?;
        let req = Command::Connect(connect_addr);
        MRPC_CTX.with(|ctx| {
            ctx.service.send_cmd(req)?;
            rx_recv_impl!(ctx.service, CompletionKind::Connect)?;
            Ok(())
        })
    }
}
