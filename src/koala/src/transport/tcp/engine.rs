use std::collections::VecDeque;
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};
use std::os::unix::io::{AsRawFd, RawFd};

use interface::{AsHandle, Handle};
use ipc::transport::tcp::{cmd, dp};
use ipc::customer::Customer;

use interface::engine::SchedulingMode;
use engine::{Engine, EngineStatus, Upgradable, Version};

use super::{DatapathError, Error};
use crate::transport::resource::ResourceTable;

pub(crate) struct State {
    listener_table: ResourceTable<TcpListener>,
    conn_table: ResourceTable<TcpStream>,
}

impl State {
    pub(crate) fn new() -> Self {
        State {
            listener_table: ResourceTable::default(),
            conn_table: ResourceTable::default(),
        }
    }
}

pub struct TransportEngine {
    pub(crate) customer: Customer<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
    pub(crate) cq_err_buffer: VecDeque<dp::Completion>,

    // this engine translate RPC messages into transport-level work requests / completions
    pub(crate) rpc_adapter: RpcAdapter,

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

        if self.customer.has_control_command()
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
        match self.customer.try_recv_cmd() {
            Ok(req) => {
                let result = self.process_cmd(&req);
                match result {
                    Ok(res) => self.customer.send_comp(cmd::Completion(Ok(res)))?,
                    Err(Error::InProgress) => {
                        // nothing to do, waiting for some network/device response
                        return Ok(Progress(0));
                    }
                    Err(e) => self.customer.send_comp(cmd::Completion(Err(e.into())))?,
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
                self.state.listener_table.open_or_create_resource(handle, listener);
                Ok(CompletionKind::Bind(handle))
            }
            Command::Accept(handle) => {
                let listener = self.state.listener_table.get(handle)?;
                let (sock, addr) = listener.accept().map_err(Error::Socket)?;
                sock.set_nonblocking(true).map_err(Error::Socket)?;
                let new_handle = sock.as_raw_fd().as_handle();
                self.state.conn_table.open_or_create_resource(new_handle, sock);
                Ok(CompletionKind::Accept(new_handle))
            }
            Command::Connect(addr) => {
                let sock = TcpStream::connect(addr).map_err(Error::Socket)?;
                sock.set_nonblocking(true).map_err(Error::Socket)?;
                let handle = sock.as_raw_fd().as_handle();
                self.state.conn_table.open_or_create_resource(handle, sock);
                Ok(CompletionKind::Connect(handle))
            }
            Command::SetSockOption(handle) => {
                unimplemented!("setsockoption");
                // Ok(CompletionKind::SetSockOption)
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
