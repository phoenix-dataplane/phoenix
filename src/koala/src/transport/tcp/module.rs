use std::collections::VecDeque;
use std::fs;
use std::mem;
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{SocketAddr, UCred};
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::Result;
use nix::unistd::Pid;
use uuid::Uuid;

use interface::engine::SchedulingMode;
use engine::manager::RuntimeManager;
use ipc;
use ipc::transport::tcp::{cmd, dp, control_plane};
use ipc::unix::DomainSocket;

use super::engine::TransportEngine;

// TODO(cjr): make these configurable, see koala.toml
const DP_WQ_DEPTH: usize = 32;
const DP_CQ_DEPTH: usize = 32;

pub(crate) struct TransportEngineBuilder {
    client_path: PathBuf,
    client_pid: Pid,
    sock: DomainSocket,
    mode: SchedulingMode,
    dp_wq_depth: usize,
    dp_cq_depth: usize,
}

impl TransportEngineBuilder {
    fn new<P: AsRef<Path>>(
        client_path: P,
        client_pid: Pid,
        sock: DomainSocket,
        mode: SchedulingMode,
    ) -> Self {
        TransportEngineBuilder {
            client_path: client_path.as_ref().to_owned(),
            client_pid,
            sock,
            mode,
            dp_wq_depth: DP_WQ_DEPTH,
            dp_cq_depth: DP_CQ_DEPTH,
        }
    }

    fn set_wq_depth(&mut self, wq_depth: usize) -> &mut Self {
        self.dp_wq_depth = wq_depth;
        self
    }

    fn set_cq_depth(&mut self, cq_depth: usize) -> &mut Self {
        self.dp_cq_depth = cq_depth;
        self
    }

    fn build(mut self) -> Result<TransportEngine> {
        // 1. connect to the client
        self.sock.connect(&self.client_path)?;

        // 2. create an IPC channel with a random name
        let (server, server_name) = ipc::OneShotServer::new()?;

        // 3. tell the name and the capacities of data path shared memory queues to the client
        let wq_cap = self.dp_wq_depth * mem::size_of::<dp::WorkRequestSlot>();
        let cq_cap = self.dp_cq_depth * mem::size_of::<dp::CompletionSlot>();

        let mut buf = bincode::serialize(&control_plane::Response(Ok(
            control_plane::ResponseKind::ConnectEngine {
                mode: self.mode,
                one_shot_name: server_name,
                wq_cap,
                cq_cap,
            },
        )))?;
        let nbytes = self.sock.send_to(buf.as_mut_slice(), &self.client_path)?;
        if nbytes != buf.len() {
            return Err(anyhow!(
                "expect to send {} bytes, but only {} was sent",
                buf.len(),
                nbytes
            ));
        }

        // 4. the client should later connect to the oneshot server, and create these channels
        // to communicate with its transport engine.
        let (_, (cmd_tx, cmd_rx)): (
            _,
            (
                ipc::IpcSender<cmd::Completion>,
                ipc::IpcReceiver<cmd::Command>,
            ),
        ) = server.accept()?;

        // 5. create data path shared memory queues
        let dp_wq = ipc::ShmReceiver::new(wq_cap)?;
        let dp_cq = ipc::ShmSender::new(cq_cap)?;

        let cmd_rx_entries = ipc::ShmObject::new(AtomicUsize::new(0))?;

        // 6. send the file descriptors back to let the client attach to these shared memory queues
        self.sock.send_fd(
            &self.client_path,
            &[
                dp_wq.memfd().as_raw_fd(),
                dp_wq.empty_signal().as_raw_fd(),
                dp_wq.full_signal().as_raw_fd(),
                dp_cq.memfd().as_raw_fd(),
                dp_cq.empty_signal().as_raw_fd(),
                dp_cq.full_signal().as_raw_fd(),
                ipc::ShmObject::memfd(&cmd_rx_entries).as_raw_fd(),
            ],
        )?;

        // 7. finally, we are done here
        Ok(TransportEngine {
            client_path: self.client_path.clone(),
            sock: self.sock,
            cmd_rx_entries,
            cmd_tx,
            cmd_rx,
            dp_wq,
            dp_cq,
            cq_err_buffer: VecDeque::new(),
            dp_spin_cnt: 0,
            backoff: 1,
            _mode: self.mode,
            state: super::engine::State::new(),
            cmd_buffer: None,
            last_cmd_ts: Instant::now(),
        })
    }
}

pub struct TransportModule {
    runtime_manager: Arc<RuntimeManager>,
}

impl TransportModule {
    pub fn new(runtime_manager: Arc<RuntimeManager>) -> Self {
        TransportModule { runtime_manager }
    }

    pub fn handle_request(
        &mut self,
        req: &control_plane::Request,
        sock: &DomainSocket,
        sender: &SocketAddr,
        cred: &UCred,
    ) -> Result<()> {
        let client_path = sender
            .as_pathname()
            .ok_or_else(|| anyhow!("peer is unnamed, something is wrong"))?;
        match req {
            control_plane::Request::NewClient(mode) => {
                self.handle_new_client(sock, client_path, *mode, cred)
            }
            _ => unreachable!("unknown req: {:?}", req),
        }
    }

    fn handle_new_client<P: AsRef<Path>>(
        &mut self,
        sock: &DomainSocket,
        client_path: P,
        mode: SchedulingMode,
        cred: &UCred,
    ) -> Result<()> {
        // 1. generate a path and bind a unix domain socket to it
        let uuid = Uuid::new_v4();
        let engine_path = PathBuf::from(format!("/tmp/koala/koala-transport-engine-{}.sock", uuid));

        if engine_path.exists() {
            // This is actually impossible using uuid.
            fs::remove_file(&engine_path)?;
        }
        let engine_sock = DomainSocket::bind(&engine_path)?;

        // 2. tell the engine's socket path to the client
        let mut buf = bincode::serialize(&control_plane::Response(Ok(
            control_plane::ResponseKind::NewClient(engine_path),
        )))?;
        let nbytes = sock.send_to(buf.as_mut_slice(), &client_path)?;
        if nbytes != buf.len() {
            return Err(anyhow!(
                "expect to send {} bytes, but only {} was sent",
                buf.len(),
                nbytes
            ));
        }

        // 3. the following part are expected to be done in the Engine's constructor.
        // the transport module is responsible for initializing and starting the transport engines
        let client_pid = Pid::from_raw(cred.pid.unwrap());
        let mut builder = TransportEngineBuilder::new(&client_path, client_pid, engine_sock, mode);
        builder.set_wq_depth(DP_WQ_DEPTH).set_cq_depth(DP_CQ_DEPTH);
        let engine = builder.build()?;

        // 5. submit the engine to a runtime
        self.runtime_manager.submit(Box::new(engine), mode);

        Ok(())
    }
}
