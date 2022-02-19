use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use ipc::mrpc::{cmd, dp};
use ipc::unix::DomainSocket;

use engine::{Engine, EngineStatus, SchedulingMode, Upgradable, Version};

use super::{DatapathError, Error};

pub struct MrpcEngine {
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

impl Engine for MrpcEngine {
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

impl MrpcEngine {
    fn flush_dp(&mut self) -> Result<Status, DatapathError> {
        unimplemented!();
    }

    fn check_cmd(&mut self) -> Result<Status, Error> {
        unimplemented!();
    }
    fn check_dp(&mut self) -> Result<Status, DatapathError> {
        unimplemented!();
    }
    fn check_cm_event(&mut self) -> Result<Status, Error> {
        unimplemented!();
    }
}
