//! Shared memory Customer implementation.
use std::cell::RefCell;
use std::env;
use std::fs;
use std::fs::File;
use std::mem;
use std::os::unix::io::FromRawFd;
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::UCred;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use crossbeam::atomic::AtomicCell;
use minstant::Instant;
use uuid::Uuid;

use serde::{Deserialize, Serialize};

use interface::engine::{EngineType, SchedulingMode};

use crate::control;
use crate::ipc_channel::{IpcReceiver, IpcSender, IpcSenderNotify};
use crate::service::MAX_MSG_LEN;
use crate::unix::DomainSocket;
use crate::{Error, IpcRecvError, IpcSendError, ShmObject, ShmReceiver, ShmSender, TryRecvError};

// TODO(cjr): make these configurable, see koala.toml
const DP_WQ_DEPTH: usize = 32;
const DP_CQ_DEPTH: usize = 32;

impl From<crate::ipc_channel::TryRecvError> for TryRecvError {
    fn from(other: crate::ipc_channel::TryRecvError) -> Self {
        use crate::ipc_channel::IpcRecvError as IRE;
        use crate::ipc_channel::TryRecvError as ITRE;
        match other {
            ITRE::Empty => TryRecvError::Empty,
            ITRE::IpcError(e) => match e {
                IRE::Disconnected => TryRecvError::Disconnected,
                IRE::Io(err) => TryRecvError::Other(Box::new(err)),
                IRE::Bincode(err) => TryRecvError::Other(Box::new(err)),
            },
        }
    }
}

impl From<crate::ipc_channel::IpcSendError> for IpcSendError {
    fn from(other: crate::ipc_channel::IpcSendError) -> Self {
        IpcSendError::Bincode(other)
    }
}

impl From<crate::ipc_channel::IpcRecvError> for IpcRecvError {
    fn from(other: crate::ipc_channel::IpcRecvError) -> Self {
        use crate::ipc_channel::IpcRecvError as IRE;
        match other {
            IRE::Disconnected => IpcRecvError::Disconnected,
            IRE::Io(err) => IpcRecvError::Other(Box::new(err)),
            IRE::Bincode(err) => IpcRecvError::Other(Box::new(err)),
        }
    }
}

/// # Safety
///
/// This is safe, because dp_wq and dp_cq requires mutable reference to access. It is impossible to
/// duplicate the mutable references.
unsafe impl<A: Sync, B: Sync, C: Sync, D: Sync> Sync for Customer<A, B, C, D> {}
unsafe impl<A: Sync, B: Sync, C: Sync, D: Sync> Sync for Service<A, B, C, D> {}

pub struct Customer<Command, Completion, WorkRequest, WorkCompletion> {
    /// This is the path of the domain socket which is client side is listening on.
    /// The mainly purpose of keeping is to send file descriptors to the client.
    client_path: PathBuf,
    sock: DomainSocket,
    cmd_rx_entries: ShmObject<AtomicUsize>,
    cmd_tx: IpcSenderNotify<Completion>,
    cmd_rx: IpcReceiver<Command>,
    dp_wq: ShmReceiver<WorkRequest>,
    dp_cq: ShmSender<WorkCompletion>,
    timer: Instant,
    fd_notifier: ShmObject<AtomicUsize>,
}

impl<Command, Completion, WorkRequest, WorkCompletion>
    Customer<Command, Completion, WorkRequest, WorkCompletion>
where
    Command: for<'de> Deserialize<'de> + Serialize,
    Completion: for<'de> Deserialize<'de> + Serialize,
    WorkRequest: Copy + zerocopy::FromBytes,
    WorkCompletion: Copy + zerocopy::AsBytes,
{
    pub fn accept<P: AsRef<Path>, Q: AsRef<Path>>(
        sock: &DomainSocket,
        client_path: P,
        mode: SchedulingMode,
        engine_path: Q,
    ) -> Result<Self, Error> {
        if engine_path.as_ref().exists() {
            // This is actually impossible using uuid.
            fs::remove_file(&engine_path)?;
        }
        let mut engine_sock = DomainSocket::bind(&engine_path)?;

        // 2. tell the engine's path to the client
        let mut buf = bincode::serialize(&control::Response(Ok(
            control::ResponseKind::NewClient(engine_path.as_ref().to_path_buf()),
        )))?;
        let nbytes = sock.send_to(buf.as_mut_slice(), &client_path)?;
        assert_eq!(
            nbytes,
            buf.len(),
            "expect to send {} bytes, but only {} was sent",
            buf.len(),
            nbytes
        );

        // 3. connect to the client
        engine_sock.connect(&client_path)?;
        // 4. create an IPC channel with a random name
        let (server, server_name) = crate::ipc_channel::OneShotServer::new()?;
        // 5. tell the name and the capacities of data path shared memory queues to the client
        let wq_cap = DP_WQ_DEPTH * mem::size_of::<WorkRequest>();
        let cq_cap = DP_CQ_DEPTH * mem::size_of::<WorkCompletion>();

        let mut buf = bincode::serialize(&control::Response(Ok(
            control::ResponseKind::ConnectEngine {
                mode,
                one_shot_name: server_name,
                wq_cap,
                cq_cap,
            },
        )))?;

        let nbytes = engine_sock.send_to(buf.as_mut_slice(), &client_path)?;
        assert_eq!(
            nbytes,
            buf.len(),
            "expect to send {} bytes, but only {} was sent",
            buf.len(),
            nbytes
        );

        // 6. the client should later connect to the oneshot server, and create these channels
        // to communicate with its transport engine.
        let (_, (cmd_tx, cmd_rx)): (_, (IpcSender<Completion>, IpcReceiver<Command>)) =
            server.accept()?;

        // 7. create data path shared memory queues
        let dp_wq = ShmReceiver::new(wq_cap)?;
        let dp_cq = ShmSender::new(cq_cap)?;

        let cmd_tx_entries = ShmObject::new(AtomicUsize::new(0))?;
        let cmd_rx_entries = ShmObject::new(AtomicUsize::new(0))?;
        let fd_notifier = ShmObject::new(AtomicUsize::new(0))?;

        // 8. send the file descriptors back to let the client attach to these shared memory queues
        engine_sock.send_fd(
            &client_path,
            &[
                dp_wq.memfd().as_raw_fd(),
                dp_wq.empty_signal().as_raw_fd(),
                dp_wq.full_signal().as_raw_fd(),
                dp_cq.memfd().as_raw_fd(),
                dp_cq.empty_signal().as_raw_fd(),
                dp_cq.full_signal().as_raw_fd(),
                ShmObject::memfd(&cmd_tx_entries).as_raw_fd(),
                ShmObject::memfd(&cmd_rx_entries).as_raw_fd(),
                ShmObject::memfd(&fd_notifier).as_raw_fd(),
            ],
        )?;

        // 9. finally, we are done here
        Ok(Self {
            client_path: client_path.as_ref().to_path_buf(),
            sock: engine_sock,
            cmd_rx_entries,
            cmd_tx: IpcSenderNotify::new(cmd_tx, cmd_tx_entries),
            cmd_rx,
            dp_wq,
            dp_cq,
            timer: Instant::now(),
            fd_notifier,
        })
    }

    #[inline]
    pub(crate) fn has_control_command(&mut self) -> bool {
        static TIMEOUT: Duration = Duration::from_millis(100);
        if self.timer.elapsed() > TIMEOUT {
            self.timer = Instant::now();
            return true;
        }
        self.cmd_rx_entries.load(Ordering::Relaxed) > 0
    }

    #[inline]
    pub(crate) fn send_fd(&self, fds: &[RawFd]) -> Result<(), Error> {
        self.fd_notifier.fetch_add(1, Ordering::AcqRel);
        Ok(self
            .sock
            .send_fd(&self.client_path, fds)
            .map_err(|e| Error::SendFd(Box::new(e)))?)
    }

    #[inline]
    pub(crate) fn try_recv_cmd(&mut self) -> Result<Command, TryRecvError> {
        if !self.has_control_command() {
            return Err(TryRecvError::Empty);
        }
        let req = self.cmd_rx.try_recv()?;
        self.cmd_rx_entries.fetch_sub(1, Ordering::Relaxed);
        Ok(req)
    }

    #[inline]
    pub(crate) fn send_comp(&self, comp: Completion) -> Result<(), Error> {
        Ok(self
            .cmd_tx
            .send(comp)
            .map_err(|e| Error::IpcSend(e.into()))?)
    }

    #[inline]
    pub(crate) fn get_avail_wr_count(&mut self) -> Result<usize, Error> {
        Ok(self.dp_wq.receiver_mut().read_count()?)
    }

    #[inline]
    pub(crate) fn get_avail_wc_slots(&mut self) -> Result<usize, Error> {
        Ok(self.dp_cq.sender_mut().write_count()?)
    }

    #[inline]
    pub(crate) fn dequeue_wr_with<F: FnOnce(*const WorkRequest, usize) -> usize>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        self.dp_wq.receiver_mut().recv(f)?;
        Ok(())
    }

    /// This will possibly trigger the eventfd.
    #[inline]
    pub(crate) fn notify_wc_with<F: FnOnce(*mut WorkCompletion, usize) -> usize>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        self.dp_cq.send_raw(f)?;
        Ok(())
    }

    /// This will bypass the eventfd, thus much faster.
    #[inline]
    pub(crate) fn enqueue_wc_with<F: FnOnce(*mut WorkCompletion, usize) -> usize>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        self.dp_cq.sender_mut().send(f)?;
        Ok(())
    }
}

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
        koala_prefix: P,
        control_path: P,
        engine_type: EngineType,
    ) -> Result<Self, Error> {
        let uuid = Uuid::new_v4();
        let arg0 = env::args().next().unwrap();
        let appname = Path::new(&arg0).file_name().unwrap().to_string_lossy();

        let sock_path = koala_prefix
            .as_ref()
            .join(format!("koala-client-{}_{}.sock", appname, uuid));
        if sock_path.exists() {
            fs::remove_file(&sock_path).expect("remove_file");
        }
        let mut sock = DomainSocket::bind(sock_path)?;

        let req = control::Request::NewClient(SchedulingMode::Dedicate, engine_type);
        let buf = bincode::serialize(&req)?;
        assert!(buf.len() < MAX_MSG_LEN);

        let service_path = koala_prefix.as_ref().join(control_path);
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

                Ok(Self {
                    sock,
                    cmd_tx: IpcSenderNotify::new(cmd_tx1, cmd_tx_entries),
                    cmd_rx: cmd_rx2,
                    dp_wq: RefCell::new(dp_wq),
                    dp_cq: RefCell::new(dp_cq),
                    timer: AtomicCell::new(Instant::now()),
                    cmd_rx_entries,
                    fd_notifier,
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
        Ok(self.cmd_rx.recv().map_err(|e| Error::IpcRecv(e.into()))?)
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
}
