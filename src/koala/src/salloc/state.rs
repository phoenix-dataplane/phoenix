use std::collections::BTreeMap;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use nix::unistd::Pid;

use super::region::SharedRegion;
use crate::engine::EngineLocalStorage;
use crate::state_mgr::{StateManager, StateTrait};

pub(crate) struct State {
    sm: Arc<StateManager<Self>>,
    pub(crate) shared: Arc<Shared>,
}

pub(crate) struct Shared {
    pub(crate) pid: Pid,
    alive_engines: AtomicUsize,
    pub(crate) resource: Resource,
}

impl StateTrait for State {
    type Err = io::Error;
    fn new(sm: Arc<StateManager<Self>>, pid: Pid) -> Result<Self, Self::Err> {
        Ok(State {
            sm,
            shared: Arc::new(Shared {
                pid,
                alive_engines: AtomicUsize::new(0),
                resource: Resource::new(),
            }),
        })
    }
}

impl Clone for State {
    fn clone(&self) -> Self {
        self.shared.alive_engines.fetch_add(1, Ordering::AcqRel);
        State {
            sm: Arc::clone(&self.sm),
            shared: Arc::clone(&self.shared),
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        let was_last = self.shared.alive_engines.fetch_sub(1, Ordering::AcqRel) == 1;
        if was_last {
            let _ = self.sm.states.lock().remove(&self.shared.pid);
        }
    }
}

impl State {
    #[inline]
    pub(crate) fn resource(&self) -> &Resource {
        &self.shared.resource
    }
}

pub(crate) struct Resource {
    // TODO(wyj): apply the alignment trick and replace the BTreeMap here.
    pub(crate) mr_table: spin::Mutex<BTreeMap<usize, SharedRegion>>,
}

unsafe impl EngineLocalStorage for Resource {
    #[inline]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Resource {
    fn new() -> Self {
        Self {
            mr_table: spin::Mutex::new(BTreeMap::default()),
        }
    }
}
