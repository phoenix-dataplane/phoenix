use std::time::{Duration, Instant};

use ipc::mrpc::{cmd, control_plane, dp};
use ipc::service::Service;

use engine::{Engine, EngineStatus, Upgradable, Version, Vertex};
use interface::engine::SchedulingMode;

use super::{DatapathError, Error};
use crate::node::Node;

pub struct RpcAdapterEngine {
    pub(crate) service:
        Service<cmd::Command, cmd::Completion, dp::WorkRequestSlot, dp::CompletionSlot>,
    node: Node,

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

impl Upgradable for RpcAdapterEngine {
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

impl Vertex for RpcAdapterEngine {
    crate::impl_vertex_for_engine!(node);
}

impl Engine for RpcAdapterEngine {
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

        if self.node.has_control_command()
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
