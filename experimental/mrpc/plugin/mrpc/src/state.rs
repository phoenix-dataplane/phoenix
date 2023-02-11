use std::io;
use std::sync::Arc;

use phoenix_common::state_mgr::{Pid, ProcessShared};

pub(crate) struct State {
    pub(crate) _shared: Arc<Shared>,
}

impl State {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        State { _shared: shared }
    }
}

#[allow(clippy::manual_non_exhaustive)]
pub struct Shared {
    pub pid: Pid,
    _other_state: (),
}

impl ProcessShared for Shared {
    type Err = io::Error;

    fn new(pid: Pid) -> io::Result<Self> {
        let shared = Shared {
            pid,
            _other_state: (),
        };
        Ok(shared)
    }
}
