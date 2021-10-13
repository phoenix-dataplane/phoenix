use experimental::command::{Command, ControlMessage};
use std::io;
use std::os::unix::net::UnixDatagram;
use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Failed to connect to the control plane: {0}")]
    FailedToConnect(io::Error),
}

pub struct QpAttrMask(u32);
pub struct QpAttr {}

pub fn query_qp(attr_mask: QpAttrMask) -> Result<QpAttr> {
    shmwq.push(attr_mask);
    let ret = shmcq.wait_and_pop(attr_mask);
    ret
}

pub fn koala_register() -> Result<UnixDatagram, Error> {
    let uuid = Uuid::new();
    let appname = std::env::Args::next().unwrap();
    let sock = UnixDatagram::bind("/tmp/koala-{}_{}.sock".format(appname, uuid));
    sock.connect("/tmp/koala-control.sock")
        .map_err(|e| Error::FailedToConnect(e))?;
    let msg = ControlMessage {
        cmd: Command::Connect,
    };

    let tx = ipc_channel::ipc::connect("/tmp/koala-control.sock");
    tx.send(msg);
    sock.sendto(msg.serialize);
    let mut buf = vec![0; 4096];
    let (count, address) = socket.recv_from(&mut buf)?;
    return ControlMessage::deserialize(buf, count);
}
