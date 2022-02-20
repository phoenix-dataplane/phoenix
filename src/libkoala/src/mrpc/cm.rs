use std::net::ToSocketAddrs;

use ipc::mrpc::cmd::{Command, CompletionKind};
use ipc::mrpc::control_plane::TransportType;

use crate::mrpc::MRPC_CTX;
use crate::Error;

pub struct MrpcStub {}

impl MrpcStub {
    pub fn set_transport(transport_type: TransportType) -> Result<(), Error> {
        let req = Command::SetTransport(transport_type);
        MRPC_CTX.with(|ctx| {
            ctx.cmd_tx.send(req)?;
            match ctx.cmd_rx.recv().map_err(Error::IpcRecv)?.0 {
                Ok(CompletionKind::SetTransport) => Ok(()),
                Err(e) => Err(Error::Interface("set_transport", e)),
                _ => panic!(""),
            }
        })
    }

    pub fn connect<A: ToSocketAddrs>(&self, addr: A) -> Result<(), Error> {
        let connect_addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or(Error::NoAddrResolved)?;
        let req = Command::Connect(connect_addr);
        MRPC_CTX.with(|ctx| {
            ctx.cmd_tx.send(req)?;
            match ctx.cmd_rx.recv().map_err(Error::IpcRecv)?.0 {
                Ok(CompletionKind::Connect) => Ok(()),
                Err(e) => Err(Error::Interface("connect", e)),
                _ => panic!(""),
            }
        })
    }
}
