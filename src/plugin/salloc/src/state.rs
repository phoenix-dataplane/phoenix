use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;

use nix::unistd::Pid;

use crate::region::AddressMediator;

use super::region::SharedRegion;
use phoenix::state_mgr::ProcessShared;

pub struct State {
    pub(crate) shared: Arc<Shared>,
    pub addr_mediator: Arc<AddressMediator>,
}

impl State {
    pub fn new(shared: Arc<Shared>, mediator: Arc<AddressMediator>) -> Self {
        State {
            shared,
            addr_mediator: mediator,
        }
    }
}

impl State {
    #[inline]
    pub fn resource(&self) -> &Resource {
        &self.shared.resource
    }
}

pub struct Shared {
    pub pid: Pid,
    pub resource: Resource,
}

impl ProcessShared for Shared {
    type Err = io::Error;

    fn new(pid: Pid) -> io::Result<Self> {
        let shared = Shared {
            pid,
            resource: Resource::new(),
        };
        Ok(shared)
    }
}

pub struct Resource {
    // TODO(wyj): apply the alignment trick and replace the BTreeMap here.
    pub(crate) mr_table: spin::Mutex<BTreeMap<usize, SharedRegion>>,
}

impl Resource {
    fn new() -> Self {
        Self {
            mr_table: spin::Mutex::new(BTreeMap::default()),
        }
    }
}
