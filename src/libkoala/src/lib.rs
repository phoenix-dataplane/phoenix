#![feature(negative_impls)]
#![feature(peer_credentials_unix_socket)]
use fnv::FnvHashMap as HashMap;
use std::borrow::Borrow;
use std::cell::RefCell;
use std::env;
use std::fs::File;
use std::io;
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UCred;
use std::path::Path;

use lazy_static::lazy_static;
use thiserror::Error;
use uuid::Uuid;

use engine::SchedulingMode;
use ipc::unix::DomainSocket;
use ipc::{self, cmd, dp};

pub mod cm;
mod fp;
pub mod verbs;

// TODO(cjr): make this configurable, see koala.toml
const KOALA_TRANSPORT_PATH: &str = "/tmp/libkoala_xinhao.sock";

const MAX_MSG_LEN: usize = 65536;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO Error {0}")]
    Io(#[from] io::Error),
    #[error("Bincode error: {0}")]
    Bincode(#[from] bincode::Error),
    #[error("IPC send error: {0}")]
    IpcSend(ipc::Error),
    #[error("IPC recv error")]
    IpcRecv(ipc::IpcError),
    #[error("Interface error {0}: {1}")]
    Interface(&'static str, interface::Error),
    #[error("DomainSocket error: {0}")]
    UnixDomainSocket(#[from] ipc::unix::Error),
    #[error("Shared memory queue error: {0}")]
    ShmIpc(#[from] ipc::ShmIpcError),
    #[error("Shared memory queue ringbuf error: {0}")]
    ShmRingbuf(#[from] ipc::ShmRingbufError),
    #[error("ShmObject error: {0}")]
    ShmObj(#[from] ipc::shm::Error),
    #[error("No address is resolved")]
    NoAddrResolved,
    #[error("Expect a credential from the peer")]
    EmptyCredential,
    #[error("Credential mismatch {0:?} vs {1:?}")]
    CredentialMismatch(UCred, UCred),
    #[error("Connect failed: {0}")]
    Connect(interface::Error),
}

thread_local! {
    pub(crate) static KL_CTX: Context = Context::register().expect("koala transport register failed");
}

// NOTE(cjr): Will lazy_static affects the performance?
lazy_static! {
    // A cq can be created by calling create_cq, but it can also come from create_ep
    pub(crate) static ref CQ_BUFFERS: spin::Mutex<HashMap<interface::CompletionQueue, verbs::CqBuffer>> =
        spin::Mutex::new(HashMap::default());
}

pub struct Context {
    sock: DomainSocket,
    cmd_tx: ipc::IpcSenderNotify<cmd::Request>,
    cmd_rx: ipc::IpcReceiver<cmd::Response>,
    dp_wq: RefCell<ipc::ShmSender<dp::WorkRequestSlot>>,
    dp_cq: RefCell<ipc::ShmReceiver<dp::CompletionSlot>>,
    // cq_buffers: RefCell<HashMap<interface::CompletionQueue, verbs::CqBuffer>>,
}

impl Context {
    fn check_credential(sock: &DomainSocket, cred: Option<UCred>) -> Result<(), Error> {
        let peer_cred = sock.peer_cred()?;
        match cred {
            Some(cred) if peer_cred == cred => Ok(()),
            Some(cred) => Err(Error::CredentialMismatch(cred, peer_cred)),
            None => Err(Error::EmptyCredential),
        }
    }

    fn register() -> Result<Context, Error> {
        let uuid = Uuid::new_v4();
        let arg0 = env::args().next().unwrap();
        let appname = Path::new(&arg0).file_name().unwrap().to_string_lossy();
        let sock_path = format!("/tmp/koala/koala-client-{}_{}.sock", appname, uuid);
        let mut sock = DomainSocket::bind(sock_path)?;

        let req = cmd::Request::NewClient(SchedulingMode::Dedicate);
        let buf = bincode::serialize(&req)?;
        assert!(buf.len() < MAX_MSG_LEN);
        sock.send_to(&buf, KOALA_TRANSPORT_PATH)?;

        // receive NewClient response
        let mut buf = vec![0u8; 128];
        let (_, sender) = sock.recv_from(buf.as_mut_slice())?;
        assert_eq!(sender.as_pathname(), Some(Path::new(KOALA_TRANSPORT_PATH)));
        let res: cmd::Response = bincode::deserialize(&buf)?;

        // return the internal error
        let res = res.0.map_err(|e| Error::Interface("NewClient", e))?;

        match res {
            cmd::ResponseKind::NewClient(engine_path) => {
                sock.connect(engine_path)?;
            }
            _ => panic!("unexpected response: {:?}", res),
        }

        // connect to the engine, setup a bunch of channels and shared memory queues
        let mut buf = vec![0u8; 128];
        let (_nbytes, _sender, cred) = sock.recv_with_credential_from(buf.as_mut_slice())?;
        Self::check_credential(&sock, cred)?;
        let res: cmd::Response = bincode::deserialize(&buf)?;

        // return the internal error
        let res = res
            .0
            .map_err(|e| Error::Interface("ConnectEngine", e))?;

        match res {
            cmd::ResponseKind::ConnectEngine(mode, server_name, wq_cap, cq_cap) => {
                assert_eq!(mode, SchedulingMode::Dedicate);
                let (cmd_tx1, cmd_rx1): (
                    ipc::IpcSender<cmd::Request>,
                    ipc::IpcReceiver<cmd::Request>,
                ) = ipc::channel()?;
                let (cmd_tx2, cmd_rx2): (
                    ipc::IpcSender<cmd::Response>,
                    ipc::IpcReceiver<cmd::Response>,
                ) = ipc::channel()?;
                let tx0 = ipc::IpcSender::connect(server_name)?;
                tx0.send((cmd_tx2, cmd_rx1))?;

                // receive file descriptors to attach to the shared memory queues
                let (fds, cred) = sock.recv_fd()?;
                Self::check_credential(&sock, cred)?;
                assert_eq!(fds.len(), 7);
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
                let cmd_notify_memfd = unsafe { File::from_raw_fd(fds[6]) };
                // attach to the shared memories
                let dp_wq =
                    ipc::ShmSender::open(wq_cap, wq_memfd, wq_empty_signal, wq_full_signal)?;
                let dp_cq =
                    ipc::ShmReceiver::open(cq_cap, cq_memfd, cq_empty_signal, cq_full_signal)?;

                let entries = ipc::ShmObject::open(cmd_notify_memfd)?;

                Ok(Context {
                    sock,
                    cmd_tx: ipc::IpcSenderNotify::new(cmd_tx1, entries),
                    cmd_rx: cmd_rx2,
                    dp_wq: RefCell::new(dp_wq),
                    dp_cq: RefCell::new(dp_cq),
                    // cq_buffers: RefCell::new(HashMap::default()),
                })
            }
            _ => panic!("unexpected response: {:?}", res),
        }
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! _rx_recv_impl {
    ($rx:expr, $resp:path) => {
        match $rx.recv().map_err(Error::IpcRecv)?.0 {
            Ok($resp) => Ok(()),
            Err(e) => Err(Error::Interface(stringify!($resp), e)),
            otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
        }
    };
    ($rx:expr, $resp:path, $ok_block:block) => {
        match $rx.recv().map_err(Error::IpcRecv)?.0 {
            Ok($resp) => $ok_block,
            Err(e) => Err(Error::Interface(stringify!($resp), e)),
            otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
        }
    };
    ($rx:expr, $resp:path, $inst:ident, $ok_block:block) => {
        match $rx.recv().map_err(Error::IpcRecv)?.0 {
            Ok($resp($inst)) => $ok_block,
            Err(e) => Err(Error::Interface(stringify!($resp), e)),
            otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
        }
    };
    ($rx:expr, $resp:path, $ok_block:block, $err:ident, $err_block:block) => {
        match $rx.recv().map_err(Error::IpcRecv)?.0 {
            Ok($resp) => $ok_block,
            Err($err) => $err_block,
            otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
        }
    };
}

pub(crate) use _rx_recv_impl as rx_recv_impl;

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
