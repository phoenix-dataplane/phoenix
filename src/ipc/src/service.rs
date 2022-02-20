//! xatu OS service
use std::cell::RefCell;
use std::env;
use std::fs::File;
use std::io;
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UCred;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use interface::engine::{SchedulingMode, EngineType};

use crate::control;
use crate::unix::DomainSocket;
use crate::{IpcReceiver, IpcSender, IpcSenderNotify, ShmObject, ShmReceiver, ShmSender};

const MAX_MSG_LEN: usize = 65536;
const KOALA_PATH: &str = "/tmp/cjr/koala/koala-control.sock";

#[derive(Debug, Error)]
pub enum Error {
    #[error("IO Error {0}")]
    Io(#[from] io::Error),
    #[error("Bincode error: {0}")]
    Bincode(#[from] bincode::Error),
    #[error("IPC send error: {0}")]
    IpcSend(crate::Error),
    #[error("IPC recv error")]
    IpcRecv(crate::IpcError),
    #[error("Interface error {0}: {1}")]
    Interface(&'static str, interface::Error),
    #[error("Control plane error {0}: {1}")]
    ControlPlane(&'static str, interface::Error),
    #[error("DomainSocket error: {0}")]
    UnixDomainSocket(#[from] crate::unix::Error),
    #[error("Shared memory queue error: {0}")]
    ShmIpc(#[from] crate::ShmIpcError),
    #[error("Shared memory queue ringbuf error: {0}")]
    ShmRingbuf(#[from] crate::ShmRingbufError),
    #[error("ShmObject error: {0}")]
    ShmObj(#[from] crate::shm::Error),
    #[error("Expect a credential from the peer")]
    EmptyCredential,
    #[error("Credential mismatch {0:?} vs {1:?}")]
    CredentialMismatch(UCred, UCred),
}

pub struct Service<Command, Completion, WorkRequest, WorkCompletion> {
    sock: DomainSocket,
    cmd_tx: IpcSenderNotify<Command>,
    cmd_rx: IpcReceiver<Completion>,
    dp_wq: RefCell<ShmSender<WorkRequest>>,
    dp_cq: RefCell<ShmReceiver<WorkCompletion>>,
}

impl<Command, Completion, WorkRequest, WorkCompletion>
    Service<Command, Completion, WorkRequest, WorkCompletion>
where
    Command: for<'de> Deserialize<'de> + Serialize,
    Completion: for<'de> Deserialize<'de> + Serialize,
    WorkRequest: Copy + zerocopy::AsBytes,
    WorkCompletion: Copy + zerocopy::FromBytes,
{
    fn check_credential(sock: &DomainSocket, cred: Option<UCred>) -> Result<(), Error> {
        let peer_cred = sock.peer_cred()?;
        match cred {
            Some(cred) if peer_cred == cred => Ok(()),
            Some(cred) => Err(Error::CredentialMismatch(cred, peer_cred)),
            None => Err(Error::EmptyCredential),
        }
    }

    fn register(engine_type: EngineType) -> Result<Self, Error> {
        let uuid = Uuid::new_v4();
        let arg0 = env::args().next().unwrap();
        let appname = Path::new(&arg0).file_name().unwrap().to_string_lossy();
        let sock_path = format!("/tmp/koala/koala-client-{}_{}.sock", appname, uuid);
        let mut sock = DomainSocket::bind(sock_path)?;

        let req = control::Request::NewClient(SchedulingMode::Dedicate, engine_type);
        let buf = bincode::serialize(&req)?;
        assert!(buf.len() < MAX_MSG_LEN);
        sock.send_to(&buf, KOALA_PATH)?;

        // receive NewClient response
        let mut buf = vec![0u8; 128];
        let (_, sender) = sock.recv_from(buf.as_mut_slice())?;
        assert_eq!(sender.as_pathname(), Some(Path::new(KOALA_PATH)));
        let res: control::Response = bincode::deserialize(&buf)?;

        // return the internal error
        let res = res.0.map_err(|e| Error::ControlPlane("NewClient", e))?;

        match res {
            control::ResponseKind::NewClient(engine_path) => {
                sock.connect(engine_path)?;
            }
            _ => panic!("unexpected response: {:?}", res),
        }

        // connect to the engine, setup a bunch of channels and shared memory queues
        let mut buf = vec![0u8; 128];
        let (_nbytes, _sender, cred) = sock.recv_with_credential_from(buf.as_mut_slice())?;
        Self::check_credential(&sock, cred)?;
        let res: control::Response = bincode::deserialize(&buf)?;

        // return the internal error
        let res = res.0.map_err(|e| Error::ControlPlane("ConnectEngine", e))?;

        match res {
            control::ResponseKind::ConnectEngine {
                mode,
                one_shot_name: server_name,
                wq_cap,
                cq_cap,
            } => {
                assert_eq!(mode, SchedulingMode::Dedicate);
                let (cmd_tx1, cmd_rx1): (IpcSender<Command>, IpcReceiver<Command>) =
                    crate::channel()?;
                let (cmd_tx2, cmd_rx2): (IpcSender<Completion>, IpcReceiver<Completion>) =
                    crate::channel()?;
                let tx0 = IpcSender::connect(server_name)?;
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
                let dp_wq = ShmSender::<WorkRequest>::open(
                    wq_cap,
                    wq_memfd,
                    wq_empty_signal,
                    wq_full_signal,
                )?;
                let dp_cq = ShmReceiver::<WorkCompletion>::open(
                    cq_cap,
                    cq_memfd,
                    cq_empty_signal,
                    cq_full_signal,
                )?;

                let entries = ShmObject::open(cmd_notify_memfd)?;

                Ok(Service {
                    sock,
                    cmd_tx: IpcSenderNotify::new(cmd_tx1, entries),
                    cmd_rx: cmd_rx2,
                    dp_wq: RefCell::new(dp_wq),
                    dp_cq: RefCell::new(dp_cq),
                })
            }
            _ => panic!("unexpected response: {:?}", res),
        }
    }
}
