#![feature(negative_impls)]
use fnv::FnvHashMap as HashMap;
use std::borrow::Borrow;
use std::cell::RefCell;
use std::env;
use std::fs::File;
use std::io;
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UnixDatagram;
use std::path::Path;

use thiserror::Error;
use uuid::Uuid;

use engine::SchedulingMode;
use ipc::{self, cmd, dp};

pub mod cm;
mod fp;
pub mod verbs;

// TODO(cjr): make this configurable, see koala.toml
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
    #[error("{0}")]
    SendFd(#[from] ipc::unix::Error),
    #[error("Shared memory queue error: {0}")]
    ShmIpc(#[from] ipc::ShmIpcError),
}

thread_local! {
    pub(crate) static KL_CTX: Context = Context::register().expect("koala transport register failed");
}

pub struct Context {
    sock: UnixDatagram,
    cmd_tx: ipc::Sender<cmd::Request>,
    cmd_rx: ipc::Receiver<cmd::Response>,
    dp_wq: RefCell<ipc::ShmSender<dp::WorkRequestSlot>>,
    dp_cq: RefCell<ipc::ShmReceiver<dp::CompletionSlot>>,
    // A cq can be created by calling create_cq, but it can also come from create_ep
    cq_buffers: RefCell<HashMap<interface::CompletionQueue, verbs::CqBuffer>>,
}

impl Context {
    fn register() -> Result<Context, Error> {
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
            cmd::ResponseKind::NewClient(mode, server_name, wq_cap, cq_cap) => {
                assert_eq!(mode, SchedulingMode::Dedicate);
                let (cmd_tx1, cmd_rx1): (ipc::Sender<cmd::Request>, ipc::Receiver<cmd::Request>) =
                    ipc::channel()?;
                let (cmd_tx2, cmd_rx2): (ipc::Sender<cmd::Response>, ipc::Receiver<cmd::Response>) =
                    ipc::channel()?;
                let tx0 = ipc::Sender::connect(server_name)?;
                tx0.send((cmd_tx2, cmd_rx1))?;

                // receive file descriptors to attach to the shared memory queues
                let fds = ipc::recv_fd(&sock)?;
                assert_eq!(fds.len(), 6);
                let (wq_memfd, wq_empty_signal, wq_full_signal) = unsafe {
                    (
                        File::from_raw_fd(fds[0]),
                        File::from_raw_fd(fds[1]),
                        File::from_raw_fd(fds[2]),
                    )
                };
                let (cq_memfd, cq_empty_signal, cq_full_signal) = unsafe {
                    (
                        File::from_raw_fd(fds[3]),
                        File::from_raw_fd(fds[4]),
                        File::from_raw_fd(fds[5]),
                    )
                };
                // attach to the shared memories
                let dp_wq =
                    ipc::ShmSender::open(wq_cap, wq_memfd, wq_empty_signal, wq_full_signal)?;
                let dp_cq =
                    ipc::ShmReceiver::open(cq_cap, cq_memfd, cq_empty_signal, cq_full_signal)?;

                Ok(Context {
                    sock,
                    cmd_tx: cmd_tx1,
                    cmd_rx: cmd_rx2,
                    dp_wq: RefCell::new(dp_wq),
                    dp_cq: RefCell::new(dp_cq),
                    cq_buffers: RefCell::new(HashMap::default()),
                })
            }
            _ => panic!("unexpected response: {:?}", res),
        }
    }
}

// Get an owned structure from a borrow
pub trait FromBorrow<Borrowed> {
    fn from_borrow<T: Borrow<Borrowed>>(borrow: &T) -> Self;
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
