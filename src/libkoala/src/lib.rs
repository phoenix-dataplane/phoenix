use std::borrow::Borrow;
use std::env;
use std::io;
use std::ops::Range;
use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::rc::Rc;

use thiserror::Error;
use uuid::Uuid;

use engine::SchedulingMode;
use ipc::{self, cmd, dp};

pub mod cm;
mod fp;
pub mod verbs;

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

impl Context {
    pub fn register() -> Result<Rc<Context>, Error> {
        let uuid = Uuid::new_v4();
        let arg0 = env::args().next().unwrap();
        let appname = Path::new(&arg0).file_name().unwrap().to_string_lossy();
        let sock_path = format!("/tmp/koala/koala-client-{}_{}.sock", appname, uuid);
        let sock = UnixDatagram::bind(sock_path)?;

        let req = cmd::Request::NewClient(SchedulingMode::Dedicate);
        let buf = bincode::serialize(&req)?;
        assert!(buf.len() < MAX_MSG_LEN);
        sock.send_to(&buf, KOALA_TRANSPORT_PATH)?;

        let mut buf = vec![0u8; 128];
        let (_, sender) = sock.recv_from(buf.as_mut_slice())?;
        assert_eq!(sender.as_pathname(), Some(Path::new(KOALA_TRANSPORT_PATH)));
        let res: cmd::Response = bincode::deserialize(&buf)?;

        let res = res.0?; // return the internal error

        match res {
            cmd::ResponseKind::NewClient(mode, server_name) => {
                assert_eq!(mode, SchedulingMode::Dedicate);
                let (cmd_tx1, cmd_rx1): (ipc::Sender<cmd::Request>, ipc::Receiver<cmd::Request>) =
                    ipc::channel()?;
                let (cmd_tx2, cmd_rx2): (ipc::Sender<cmd::Response>, ipc::Receiver<cmd::Response>) =
                    ipc::channel()?;
                let (dp_tx1, dp_rx1): (ipc::Sender<dp::Request>, ipc::Receiver<dp::Request>) =
                    ipc::channel()?;
                let (dp_tx2, dp_rx2): (ipc::Sender<dp::Response>, ipc::Receiver<dp::Response>) =
                    ipc::channel()?;
                let tx0 = ipc::Sender::connect(server_name)?;
                tx0.send((cmd_tx2, cmd_rx1, dp_tx2, dp_rx1))?;

                Ok(Rc::new(Context {
                    sock,
                    cmd_tx: cmd_tx1,
                    cmd_rx: cmd_rx2,
                    dp_tx: dp_tx1,
                    dp_rx: dp_rx2,
                }))
            }
            _ => panic!("unexpected response: {:?}", res),
        }
    }
}

// Get an owned structure from a borrow
pub trait FromBorrow<Borrowed> {
    fn from_borrow<T: Borrow<Borrowed>>(borrow: &T) -> Self;
}

#[inline]
pub(crate) fn slice_to_range<T>(s: &[T]) -> Range<u64> {
    let r = s.as_ptr_range();
    Range {
        start: r.start as u64,
        end: r.end as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pingpong() {
        let ctx = Context::register().unwrap();
        ctx.cmd_tx.send(cmd::Request::Hello(42)).unwrap();
        let res = ctx.cmd_rx.recv().unwrap();

        match res.0 {
            Ok(cmd::ResponseKind::HelloBack(42)) => {}
            _ => panic!("wrong response"),
        }
    }
}
