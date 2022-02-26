//! xatu OS customer
use std::fs;
use std::mem;
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};

use interface::engine::SchedulingMode;

use crate::control;
pub use crate::service::Error;
use crate::unix::DomainSocket;
use crate::{IpcReceiver, IpcSender, ShmObject, ShmReceiver, ShmSender};

// TODO(cjr): make these configurable, see koala.toml
const DP_WQ_DEPTH: usize = 32;
const DP_CQ_DEPTH: usize = 32;

pub struct Customer<Command, Completion, WorkRequest, WorkCompletion> {
    /// This is the path of the domain socket which is client side is listening on.
    /// The mainly purpose of keeping is to send file descriptors to the client.
    client_path: PathBuf,
    sock: DomainSocket,
    cmd_rx_entries: ShmObject<AtomicUsize>,
    cmd_tx: IpcSender<Completion>,
    cmd_rx: IpcReceiver<Command>,
    dp_wq: ShmReceiver<WorkRequest>,
    dp_cq: ShmSender<WorkCompletion>,
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
        // 1. generate a path and bind a unix domain socket to it
        // let uuid = Uuid::new_v4();
        // let engine_path = PathBuf::from(format!("/tmp/koala/koala-transport-engine-{}.sock", uuid));

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
        let (server, server_name) = crate::OneShotServer::new()?;
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

        let cmd_rx_entries = ShmObject::new(AtomicUsize::new(0))?;

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
                ShmObject::memfd(&cmd_rx_entries).as_raw_fd(),
            ],
        )?;

        // 9. finally, we are done here
        Ok(Self {
            client_path: client_path.as_ref().to_path_buf(),
            sock: engine_sock,
            cmd_rx_entries,
            cmd_tx,
            cmd_rx,
            dp_wq,
            dp_cq,
        })
    }

    #[inline]
    pub fn has_control_command(&self) -> bool {
        self.cmd_rx_entries.load(Ordering::Relaxed) > 0
    }

    #[inline]
    pub fn send_fd(&self, fds: &[RawFd]) -> Result<(), crate::unix::Error> {
        Ok(self.sock.send_fd(&self.client_path, fds)?)
    }

    #[inline]
    pub fn try_recv_cmd(&self) -> Result<Command, crate::TryRecvError> {
        let req = self.cmd_rx.try_recv()?;
        self.cmd_rx_entries.fetch_sub(1, Ordering::Relaxed);
        Ok(req)
    }

    #[inline]
    pub fn send_comp(&self, comp: Completion) -> Result<(), Error> {
        Ok(self.cmd_tx.send(comp)?)
    }

    #[inline]
    pub fn get_avail_wr_count(&mut self) -> Result<usize, Error> {
        Ok(self.dp_wq.receiver_mut().read_count()?)
    }

    #[inline]
    pub fn get_avail_wc_slots(&mut self) -> Result<usize, Error> {
        Ok(self.dp_cq.sender_mut().write_count()?)
    }

    #[inline]
    pub fn dequeue_wr_with<F: FnOnce(*const WorkRequest, usize) -> usize>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        self.dp_wq.receiver_mut().recv(f)?;
        Ok(())
    }

    /// This will possibly trigger the eventfd.
    #[inline]
    pub fn notify_wc_with<F: FnOnce(*mut WorkCompletion, usize) -> usize>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        self.dp_cq.send_raw(f)?;
        Ok(())
    }

    /// This will bypass the eventfd, thus much faster.
    #[inline]
    pub fn enqueue_wc_with<F: FnOnce(*mut WorkCompletion, usize) -> usize>(
        &mut self,
        f: F,
    ) -> Result<(), Error> {
        self.dp_cq.sender_mut().send(f)?;
        Ok(())
    }
}
