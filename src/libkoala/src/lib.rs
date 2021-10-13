use std::env;
use std::io;
use std::os::unix::net::UnixDatagram;
use std::path::Path;

use thiserror::Error;
use uuid::Uuid;

// use experimental::command::{Command, ControlMessage};
use engine::SchedulingMode;
use ipc::{
    self,
    cmd::{Request, Response},
};

const KOALA_TRANSPORT_PATH: &str = "/tmp/koala/koala-transport.sock";
const MAX_MSG_LEN: usize = 65536;

#[derive(Error, Debug)]
pub enum Error {
    // #[error("Failed to bind: {0}")]
    // UnixDomainBind(io::Error),
    // #[error("Failed to connect to the control plane: {0}")]
    // FailedToConnect(io::Error),
    #[error("IO Error {0}")]
    Io(#[from] io::Error),
    #[error("Bincode error: {0}")]
    Bincode(#[from] bincode::Error),
}

pub struct Context {
    sock: UnixDatagram,
    tx: ipc::Sender<Request>,
    rx: ipc::Receiver<Response>,
}

pub struct QpAttrMask(u32);
pub struct QpAttr {}

pub fn query_qp(attr_mask: QpAttrMask) -> Result<QpAttr, Error> {
    unimplemented!();
    // shmwq.push(attr_mask);
    // let ret = shmcq.wait_and_pop(attr_mask);
    // ret
}

pub fn koala_register() -> Result<Context, Error> {
    let uuid = Uuid::new_v4();
    let arg0 = env::args().next().unwrap();
    let appname = Path::new(&arg0).file_name().unwrap().to_string_lossy();
    let sock_path = format!("/tmp/koala/koala-client-{}_{}.sock", appname, uuid);
    let sock = UnixDatagram::bind(sock_path)?;
    sock.connect(KOALA_TRANSPORT_PATH)?;

    let req = Request::NewClient(SchedulingMode::Dedicate);
    let buf = bincode::serialize(&req)?;
    assert!(buf.len() < MAX_MSG_LEN);
    sock.send(&buf)?;

    let mut buf = vec![0u8; 128];
    sock.recv(buf.as_mut_slice())?;
    let res: Response = bincode::deserialize(&buf)?;

    assert!(matches!(res, Response::NewClient(..)));

    match res {
        Response::NewClient(mode, server_name) => {
            assert_eq!(mode, SchedulingMode::Dedicate);
            let (tx1, rx1): (ipc::Sender<Request>, ipc::Receiver<Request>) =
                ipc::channel().unwrap();
            let (tx2, rx2): (ipc::Sender<Response>, ipc::Receiver<Response>) =
                ipc::channel().unwrap();
            let tx0 = ipc::Sender::connect(server_name).unwrap();
            tx0.send((tx2, rx1)).unwrap();

            Ok(Context {
                sock,
                tx: tx1,
                rx: rx2,
            })
        }
        _ => panic!("unexpected response: {:?}", res),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pingpong() {
        let ctx = koala_register().unwrap();
        ctx.tx.send(Request::Hello(42)).unwrap();
        let res = ctx.rx.recv().unwrap();

        match res {
            Response::HelloBack(42) => {}
            _ => panic!("wrong response"),
        }
    }
}
