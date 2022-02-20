use std::collections::VecDeque;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use std::os::unix::io::{AsRawFd, RawFd};

use fnv::FnvHashMap as HashMap;

use interface::{AsHandle, Handle};
use ipc::transport::tcp::{cmd, dp};
use ipc::unix::DomainSocket;

use engine::{Engine, EngineStatus, SchedulingMode, Upgradable, Version};

use super::{DatapathError, Error};
use crate::transport::resource::ResourceTable;

pub(crate) struct State {
    listener_table: ResourceTable<TcpListener>,
    conn_table: ResourceTable<TcpStream>,
}

impl State {
    pub(crate) fn new() -> Self {
        State {
            listener_table: HashMap::default(),
            conn_table: HashMap::default(),
        }
    }
}

pub struct TransportEngine {
    pub(crate) client_path: PathBuf,
    pub(crate) sock: DomainSocket,
    pub(crate) cmd_rx_entries: ipc::ShmObject<AtomicUsize>,
    pub(crate) cmd_tx: ipc::IpcSender<cmd::Completion>,
    pub(crate) cmd_rx: ipc::IpcReceiver<cmd::Command>,
    pub(crate) dp_wq: ipc::ShmReceiver<dp::WorkRequestSlot>,
    pub(crate) dp_cq: ipc::ShmSender<dp::CompletionSlot>,
    pub(crate) cq_err_buffer: VecDeque<dp::Completion>,

    pub(crate) dp_spin_cnt: usize,
    pub(crate) backoff: usize,
    pub(crate) _mode: SchedulingMode,

    pub(crate) state: State,

    // bufferred control path request
    pub(crate) cmd_buffer: Option<cmd::Command>,
    // otherwise, the
    pub(crate) last_cmd_ts: Instant,
}

impl Upgradable for TransportEngine {
    fn version(&self) -> Version {
        unimplemented!();
    }

    fn check_compatible(&self, _v2: Version) -> bool {
        unimplemented!();
    }

    fn suspend(&mut self) {
        unimplemented!();
    }

    fn dump(&self) {
        unimplemented!();
    }

    fn restore(&mut self) {
        unimplemented!();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Progress(usize),
    Disconnected,
}

use Status::Progress;

impl Engine for TransportEngine {
    fn resume(&mut self) -> Result<EngineStatus, Box<dyn std::error::Error>> {
        const DP_LIMIT: usize = 1 << 17;
        const CMD_MAX_INTERVAL_MS: u64 = 1000;
        if let Status::Progress(n) = self.check_dp()? {
            if n > 0 {
                self.backoff = DP_LIMIT.min(self.backoff * 2);
            }
        }

        self.dp_spin_cnt += 1;
        if self.dp_spin_cnt < self.backoff {
            return Ok(EngineStatus::Continue);
        }

        self.dp_spin_cnt = 0;

        if self.cmd_rx_entries.load(Ordering::Relaxed) > 0
            || self.last_cmd_ts.elapsed() > Duration::from_millis(CMD_MAX_INTERVAL_MS)
        {
            self.last_cmd_ts = Instant::now();
            self.backoff = std::cmp::max(1, self.backoff / 2);
            self.flush_dp()?;
            if let Status::Disconnected = self.check_cmd()? {
                return Ok(EngineStatus::Complete);
            }
        } else {
            self.backoff = DP_LIMIT.min(self.backoff * 2);
        }

        if self.cmd_buffer.is_some() {
            self.check_cm_event()?;
        }

        Ok(EngineStatus::Continue)
    }
}

impl TransportEngine {
    fn flush_dp(&mut self) -> Result<Status, DatapathError> {
        unimplemented!();
    }

    fn check_cmd(&mut self) -> Result<Status, Error> {
        match self.cmd_rx.try_recv() {
            Ok(req) => {
                self.cmd_rx_entries.fetch_sub(1, Ordering::Relaxed);
                let result = self.process_cmd(&req);
                match result {
                    Ok(res) => self.cmd_tx.send(cmd::Completion(Ok(res)))?,
                    Err(Error::InProgress) => {
                        // nothing to do, waiting for some network/device response
                        return Ok(Progress(0));
                    }
                    Err(e) => self.cmd_tx.send(cmd::Completion(Err(e.into())))?,
                }
                Ok(Progress(1))
            }
            Err(ipc::TryRecvError::Empty) => {
                // do nothing
                Ok(Progress(0))
            }
            Err(ipc::TryRecvError::IpcError(ipc::IpcError::Disconnected)) => {
                Ok(Status::Disconnected)
            }
            Err(ipc::TryRecvError::IpcError(_e)) => Err(Error::IpcTryRecv),
        }
    }

    fn process_cmd(&mut self, req: &cmd::Command) -> Result<cmd::CompletionKind, Error> {
        use cmd::{Command, CompletionKind};
        match req {
            Command::Bind(addr) => {
                let listener = TcpListener::bind(addr).map_err(Error::Socket)?;
                // listener.set_nonblocking(true)?.map_err(Error::Socket)?;
                let handle = listener.as_raw_fd().as_handle();
                self.state.listener_table.open_or_create_resource(listener)?;
                Ok(CompletionKind::Bind(handle))
            }
            Command::Accept(handle) => {
                let listener = self.state.listener_table.get(handle)?;
                listener.accept();
            }
            Command::Connect(addr) => {}
            Command::SetSockOption(handle) => {
                Ok(CompletionKind::SetSockOption)
            }
        }
    }

    fn check_dp(&mut self) -> Result<Status, DatapathError> {
        unimplemented!();
    }

    fn check_cm_event(&mut self) -> Result<Status, Error> {
        unimplemented!();
    }
}
