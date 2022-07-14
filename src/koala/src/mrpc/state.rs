use std::sync::Arc;
use std::io;

use nix::unistd::Pid;

use crate::state_mgr::ProcessShared;

pub(crate) struct State {
    pub(crate) shared: Arc<Shared>,
}

impl State {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        State { shared }
    }
}
pub(crate) struct Shared {
    pub(crate) pid: Pid,
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
