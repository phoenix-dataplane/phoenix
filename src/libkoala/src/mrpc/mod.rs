use std::cell::RefCell;
use std::env;
use std::fs::File;
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UCred;
use std::path::Path;

use uuid::Uuid;

use engine::SchedulingMode;
use ipc;
use ipc::control;
use ipc::mrpc::{cmd, control_plane, dp};
use ipc::unix::DomainSocket;

// Re-exports
use crate::{Error, KOALA_PATH, MAX_MSG_LEN};

pub mod cm;

thread_local! {
    // Initialization is dynamically performed on the first call to with within a thread.
    pub(crate) static MRPC_CTX: Context = Context::register().expect("koala mRPC register failed");
}

pub(crate) struct Context {
    sock: DomainSocket,
    cmd_tx: ipc::IpcSenderNotify<cmd::Command>,
    cmd_rx: ipc::IpcReceiver<cmd::Completion>,
    dp_wq: RefCell<ipc::ShmSender<dp::WorkRequestSlot>>,
    dp_cq: RefCell<ipc::ShmReceiver<dp::CompletionSlot>>,
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

        let req = control_plane::Request::NewClient(SchedulingMode::Dedicate);
        let buf = bincode::serialize(&control::Request::Mrpc(req))?;
        assert!(buf.len() < MAX_MSG_LEN);
        sock.send_to(&buf, KOALA_PATH)?;

        // receive NewClient response
        let mut buf = vec![0u8; 128];
        let (_, sender) = sock.recv_from(buf.as_mut_slice())?;
        assert_eq!(sender.as_pathname(), Some(Path::new(KOALA_PATH)));
        let res: control_plane::Response = bincode::deserialize(&buf)?;

        // return the internal error
        let res = res.0.map_err(|e| Error::ControlPlane("NewClient", e))?;

        match res {
            control_plane::ResponseKind::NewClient(engine_path) => {
                sock.connect(engine_path)?;
            }
            _ => panic!("unexpected response: {:?}", res),
        }

        // connect to the engine, setup a bunch of channels and shared memory queues
        let mut buf = vec![0u8; 128];
        let (_nbytes, _sender, cred) = sock.recv_with_credential_from(buf.as_mut_slice())?;
        Self::check_credential(&sock, cred)?;
        let res: control_plane::Response = bincode::deserialize(&buf)?;

        // return the internal error
        let res = res.0.map_err(|e| Error::ControlPlane("ConnectEngine", e))?;

        match res {
            control_plane::ResponseKind::ConnectEngine {
                mode,
                one_shot_name: server_name,
                wq_cap,
                cq_cap,
            } => {
                assert_eq!(mode, SchedulingMode::Dedicate);
                let (cmd_tx1, cmd_rx1): (
                    ipc::IpcSender<cmd::Command>,
                    ipc::IpcReceiver<cmd::Command>,
                ) = ipc::channel()?;
                let (cmd_tx2, cmd_rx2): (
                    ipc::IpcSender<cmd::Completion>,
                    ipc::IpcReceiver<cmd::Completion>,
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
                })
            }
            _ => panic!("unexpected response: {:?}", res),
        }
    }
}
