use std::time::{Duration, Instant};

use ipc::customer::Customer;
use ipc::mrpc::{cmd, control_plane, dp};

use engine::{Engine, EngineStatus, Upgradable, Version};
use interface::engine::SchedulingMode;

use super::{DatapathError, Error};

pub struct MrpcEngine {
    pub(crate) customer: Customer<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,

    pub(crate) dp_spin_cnt: usize,
    pub(crate) backoff: usize,
    pub(crate) _mode: SchedulingMode,

    // state
    pub(crate) transport_type: Option<control_plane::TransportType>,

    // bufferred control path request
    pub(crate) cmd_buffer: Option<cmd::Command>,
    // otherwise, the
    pub(crate) last_cmd_ts: Instant,
}

impl Upgradable for MrpcEngine {
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

impl Engine for MrpcEngine {
    fn resume(&mut self) -> Result<EngineStatus, Box<dyn std::error::Error>> {
        const DP_LIMIT: usize = 1 << 17;
        const CMD_MAX_INTERVAL_MS: u64 = 1000;
        if let Progress(n) = self.check_dp()? {
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

impl MrpcEngine {
    fn flush_dp(&mut self) -> Result<Status, DatapathError> {
        unimplemented!();
    }

    fn check_cmd(&mut self) -> Result<Status, Error> {
        match self.customer.try_recv_cmd() {
            // handle request
            Ok(req) => {
                let result = self.process_cmd(&req);
                match result {
                    Ok(res) => self.customer.send_comp(cmd::Completion(Ok(res)))?,
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

    fn create_transport(&mut self, transport_type: control_plane::TransportType) {
        self.transport_type = Some(transport_type);
    }

    fn process_cmd(&mut self, req: &cmd::Command) -> Result<cmd::CompletionKind, Error> {
        use ipc::mrpc::cmd::{Command, CompletionKind};
        match req {
            Command::SetTransport(transport_type) => {
                if self.transport_type.is_some() {
                    Err(Error::TransportType)
                } else {
                    self.create_transport(*transport_type);
                    Ok(CompletionKind::SetTransport)
                }
            }
            Command::Connect(addr) => {
                if self.transport_type.is_none() {
                    self.create_transport(control_plane::TransportType::Socket);
                }

                Ok(CompletionKind::Connect)
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