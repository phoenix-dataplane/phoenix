//! Shared memory Customer implementation.
use std::cell::RefCell;
use std::env;
use std::fs;
use std::fs::File;
use std::os::unix::io::FromRawFd;
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::UCred;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[cfg(feature = "customer")]
use std::task::{Context, Poll};

use crossbeam::atomic::AtomicCell;
use minstant::Instant;
use uuid::Uuid;

use serde::{Deserialize, Serialize};

use uapi::engine::SchedulingHint;

use crate::control;
use crate::ipc_channel::{IpcReceiver, IpcSender, IpcSenderNotify};
use crate::unix::DomainSocket;
use crate::MAX_MSG_LEN;
use crate::{Error, ShmObject, ShmReceiver, ShmSender, TryRecvError};

pub type ShmService<A, B, C, D> = Service<A, B, C, D>;

unsafe impl<A: Sync, B: Sync, C: Sync, D: Sync> Sync for Service<A, B, C, D> {}

/// A `Service` sends Command (contorl path) and WorkRequest (datapath)
/// and reply with Completion (control path) and WorkCompletion (datapath).
///
/// The user must ensure that there is no concurrent access to this Service.
pub struct Service<Command, Completion, WorkRequest, WorkCompletion> {
    sock: DomainSocket,
    cmd_tx: IpcSenderNotify<Command>,
    cmd_rx: IpcReceiver<Completion>,
    dp_wq: RefCell<ShmSender<WorkRequest>>,
    dp_cq: RefCell<ShmReceiver<WorkCompletion>>,
    timer: AtomicCell<Instant>,
    cmd_rx_entries: ShmObject<AtomicUsize>,
    fd_notifier: ShmObject<AtomicUsize>,
    #[cfg(feature = "customer")]
    dp_cq_eventfd: async_io::Async<RawFd>,
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

    pub fn register<P: AsRef<Path>>(
        phoenix_prefix: P,
        control_path: P,
        service: String,
        hint: SchedulingHint,
        config_str: Option<&str>,
    ) -> Result<Self, Error> {
        let uuid = Uuid::new_v4();
        let arg0 = env::args().next().unwrap();
        let appname = Path::new(&arg0).file_name().unwrap().to_string_lossy();

        let sock_path = phoenix_prefix
            .as_ref()
            .join(format!("phoenix-client-{}_{}.sock", appname, uuid));
        if sock_path.exists() {
            fs::remove_file(&sock_path).expect("remove_file");
        }
        let mut sock = DomainSocket::bind(sock_path)?;

        let req = control::Request::NewClient(hint, service, config_str.map(|s| s.to_string()));
        let buf = bincode::serialize(&req)?;
        assert!(buf.len() < MAX_MSG_LEN);

        let service_path = phoenix_prefix.as_ref().join(control_path);
        sock.send_to(&buf, &service_path)?;

        // receive NewClient response
        let mut buf = vec![0u8; 128];
        let (_, sender) = sock.recv_from(buf.as_mut_slice())?;
        assert_eq!(sender.as_pathname(), Some(service_path.as_ref()));
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
                mode: _mode,
                one_shot_name: server_name,
                wq_cap,
                cq_cap,
            } => {
                // assert_eq!(mode, SchedulingMode::Dedicate);
                let (cmd_tx1, cmd_rx1): (IpcSender<Command>, IpcReceiver<Command>) =
                    crate::ipc_channel::channel()?;
                let (cmd_tx2, cmd_rx2): (IpcSender<Completion>, IpcReceiver<Completion>) =
                    crate::ipc_channel::channel()?;
                let tx0 = IpcSender::connect(server_name)?;
                tx0.send((cmd_tx2, cmd_rx1))?;

                // receive file descriptors to attach to the shared memory queues
                let (fds, cred) = sock.recv_fd()?;
                Self::check_credential(&sock, cred)?;
                assert_eq!(fds.len(), 9);
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
                let cmd_rx_notify_memfd = unsafe { File::from_raw_fd(fds[6]) };
                let cmd_tx_notify_memfd = unsafe { File::from_raw_fd(fds[7]) };
                let fd_notifier_memfd = unsafe { File::from_raw_fd(fds[8]) };

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

                let cmd_rx_entries = ShmObject::open(cmd_rx_notify_memfd)?;
                let cmd_tx_entries = ShmObject::open(cmd_tx_notify_memfd)?;
                let fd_notifier = ShmObject::open(fd_notifier_memfd)?;

                #[cfg(feature = "customer")]
                let dp_cq_eventfd = async_io::Async::new(dp_cq.empty_signal().as_raw_fd())?;

                Ok(Self {
                    sock,
                    cmd_tx: IpcSenderNotify::new(cmd_tx1, cmd_tx_entries),
                    cmd_rx: cmd_rx2,
                    dp_wq: RefCell::new(dp_wq),
                    dp_cq: RefCell::new(dp_cq),
                    timer: AtomicCell::new(Instant::now()),
                    cmd_rx_entries,
                    fd_notifier,
                    #[cfg(feature = "customer")]
                    dp_cq_eventfd,
                })
            }
            _ => panic!("unexpected response: {:?}", res),
        }
    }

    #[inline]
    pub fn recv_fd(&self) -> Result<Vec<RawFd>, Error> {
        let (fds, cred) = self.sock.recv_fd()?;
        Self::check_credential(&self.sock, cred)?;
        Ok(fds)
    }

    #[inline]
    pub fn try_recv_fd(&self) -> Result<Vec<RawFd>, Error> {
        if self.fd_notifier.load(Ordering::Relaxed) > 0 {
            self.fd_notifier.fetch_sub(1, Ordering::Relaxed);
            let (fds, cred) = self.sock.recv_fd()?;
            Self::check_credential(&self.sock, cred)?;
            Ok(fds)
        } else {
            Err(Error::TryRecvFd(TryRecvError::Empty))
        }
    }

    #[inline]
    pub fn send_cmd(&self, cmd: Command) -> Result<(), Error> {
        Ok(self.cmd_tx.send(cmd)?)
    }

    #[inline]
    pub fn recv_comp(&self) -> Result<Completion, Error> {
        self.cmd_rx.recv().map_err(|e| Error::IpcRecv(e.into()))
    }

    #[inline]
    pub fn has_recv_comp(&self) -> bool {
        static TIMEOUT: Duration = Duration::from_millis(100);
        if self.timer.load().elapsed() > TIMEOUT {
            self.timer.store(Instant::now());
            return true;
        }
        self.cmd_rx_entries.load(Ordering::Relaxed) > 0
    }

    #[inline]
    pub fn try_recv_comp(&self) -> Result<Completion, Error> {
        if !self.has_recv_comp() {
            return Err(Error::TryRecv(TryRecvError::Empty));
        }
        let comp = self
            .cmd_rx
            .try_recv()
            .map_err(|e| Error::TryRecv(e.into()))?;
        self.cmd_rx_entries.fetch_sub(1, Ordering::Relaxed);
        Ok(comp)
    }

    #[inline]
    pub fn enqueue_wr_with<F: FnOnce(*mut WorkRequest, usize) -> usize>(
        &self,
        f: F,
    ) -> Result<(), Error> {
        self.dp_wq.borrow_mut().sender_mut().send(f)?;
        Ok(())
    }

    #[inline]
    pub fn dequeue_wc_with<F: FnOnce(*const WorkCompletion, usize) -> usize>(
        &self,
        f: F,
    ) -> Result<(), Error> {
        self.dp_cq.borrow_mut().receiver_mut().recv(f)?;
        Ok(())
    }

    /// For CPU efficient scenarios.
    /// Returns
    #[cfg(feature = "customer")]
    pub fn poll_wc_readable(&self, cx: &mut Context<'_>) -> Poll<Result<bool, Error>> {
        let s = self.dp_cq.borrow_mut().receiver_mut().read_count()?;
        if s > 0 {
            return Poll::Ready(Ok(true));
        };

        match self.dp_cq_eventfd.poll_readable(cx) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(res) => res?,
        }

        // TODO(cjr): set eventfd as nonblocking and use read_with_mut() to move
        // this read operation to the background thread
        use std::io::Read;
        let mut b = [0u8; 8];
        let _ = self.dp_cq.borrow_mut().empty_signal().read(&mut b)?;

        // let s = self.dp_cq.borrow_mut().receiver_mut().read_count()?;
        Poll::Ready(Ok(true))
    }
}
