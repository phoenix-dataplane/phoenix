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
use ipc::mrpc::{cmd, control_plane, dp};
use ipc::unix::DomainSocket;

use super::engine::MrpcEngine;

const DP_WQ_DEPTH: usize = 32;
const DP_CQ_DEPTH: usize = 32;

pub(crate) struct MrpcEngineBuilder {
    client_path: PathBuf,
    client_pid: Pid,
    sock: DomainSocket,
    mode: SchedulingMode,
    transport: control_plane::TransportType,
    dp_wq_depth: usize,
    dp_cq_depth: usize,
}

impl MrpcEngineBuilder {
    fn new<P: AsRef<Path>>(
        client_path: P,
        client_pid: Pid,
        sock: DomainSocket,
        mode: SchedulingMode,
    ) -> Self {
        MrpcEngineBuilder {
            client_path: client_path.as_ref().to_owned(),
            client_pid,
            sock,
            mode,
            transport: control_plane::TransportType::Socket,
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

    fn set_transport(&mut self, transport: control_plane::TransportType) -> &mut Self {
        self.transport = transport;
        self
    }

    fn build(mut self) -> Result<MrpcEngine> {
        self.sock.connect(&self.client_path)?;
        let (server, server_name) = ipc::OneShotServer::new()?;
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

        let (_, (cmd_tx, cmd_rx)): (
            _,
            (
                ipc::IpcSender<cmd::Completion>,
                ipc::IpcReceiver<cmd::Command>,
            ),
        ) = server.accept()?;

        let dp_wq = ipc::ShmReceiver::new(wq_cap)?;
        let dp_cq = ipc::ShmSender::new(cq_cap)?;

        let cmd_rx_entries = ipc::ShmObject::new(AtomicUsize::new(0))?;

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

        // let state = STATE_MGR.get_or_create_state(self.client_pid)?;

        Ok(MrpcEngine {
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
            datapath: None,
            cmd_buffer: None,
            last_cmd_ts: Instant::now(),
        })
    }
}

pub struct MrpcModule {
    runtime_manager: Arc<RuntimeManager>,
}

impl MrpcModule {
    pub fn new(runtime_manager: Arc<RuntimeManager>) -> Self {
        MrpcModule { runtime_manager }
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
            control_plane::Request::NewClient(mode, transport) => {
                self.handle_new_client(sock, client_path, *mode, *transport, cred)
            }
            _ => unreachable!("unknown req: {:?}", req),
        }
    }

    fn handle_new_client<P: AsRef<Path>>(
        &mut self,
        sock: &DomainSocket,
        client_path: P,
        mode: SchedulingMode,
        transport: control_plane::Transport,
        cred: &UCred,
    ) -> Result<()> {
        // 1. generate a path and bind a unix domain socket to it
        let uuid = Uuid::new_v4();
        let engine_path = PathBuf::from(format!("/tmp/koala/koala-mrpc-engine-{}.sock", uuid));

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
        // the mrpc module is responsible for initializing and starting the mrpc engines
        let client_pid = Pid::from_raw(cred.pid.unwrap());
        let mut builder = MrpcEngineBuilder::new(&client_path, client_pid, engine_sock, mode);
        builder
            .set_transport(transport)
            .set_wq_depth(DP_WQ_DEPTH)
            .set_cq_depth(DP_CQ_DEPTH);
        let engine = builder.build()?;

        // 5. submit the engine to a runtime
        self.runtime_manager.submit(Box::new(engine), mode);

        Ok(())
    }
}
