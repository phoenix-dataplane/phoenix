use std::env;
use std::io;
use std::os::unix::net::UnixDatagram;
use std::path::Path;

use thiserror::Error;
use uuid::Uuid;

use engine::SchedulingMode;
use ipc::{self, cmd, dp};

pub mod cm;

const KOALA_TRANSPORT_PATH: &str = "/tmp/koala/koala-transport.sock";
const MAX_MSG_LEN: usize = 65536;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO Error {0}")]
    Io(#[from] io::Error),
    #[error("Bincode error: {0}")]
    Bincode(#[from] bincode::Error),
    #[error("IPC send error: {0}")]
    IpcSendError(ipc::Error),
    #[error("IPC recv error")]
    IpcRecvError(ipc::IpcError),
    #[error("Internal error: {0}")]
    InternalError(#[from] interface::Error),
}

pub struct Context {
    sock: UnixDatagram,
    cmd_tx: ipc::Sender<cmd::Request>,
    cmd_rx: ipc::Receiver<cmd::Response>,
    dp_tx: ipc::Sender<dp::Request>,
    dp_rx: ipc::Receiver<dp::Response>,
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
            let (cmd_tx1, cmd_rx1): (ipc::Sender<cmd::Request>, ipc::Receiver<cmd::Request>) =
                ipc::channel().unwrap();
            let (cmd_tx2, cmd_rx2): (ipc::Sender<cmd::Response>, ipc::Receiver<cmd::Response>) =
                ipc::channel().unwrap();
            let (dp_tx1, dp_rx1): (ipc::Sender<dp::Request>, ipc::Receiver<dp::Request>) =
                ipc::channel().unwrap();
            let (dp_tx2, dp_rx2): (ipc::Sender<dp::Response>, ipc::Receiver<dp::Response>) =
                ipc::channel().unwrap();
            let tx0 = ipc::Sender::connect(server_name).unwrap();
            tx0.send((cmd_tx2, cmd_rx1, dp_tx2, dp_tx1)).unwrap();

            Ok(Context {
                sock,
                cmd_tx: cmd_tx1,
                cmd_rx: cmd_rx2,
                dp_tx: dp_tx1,
                dp_rx: dp_rx2,
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
