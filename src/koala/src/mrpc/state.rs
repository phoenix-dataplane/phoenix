use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nix::unistd::Pid;

use crate::engine::EngineLocalStorage;
use crate::resource::Error as ResourceError;
use crate::state_mgr::{StateManager, StateTrait};

use super::marshal::ShmBuf;

pub(crate) struct State {
    sm: Arc<StateManager<Self>>,
    pub(crate) shared: Arc<Shared>,
}

pub(crate) struct Shared {
    pub(crate) pid: Pid,
    alive_engines: AtomicUsize,
    resource: Resource,
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
    // map from local addr to app addr
    pub(crate) addr_map: spin::Mutex<BTreeMap<usize, ShmBuf>>,
}

unsafe impl EngineLocalStorage for Resource {
    #[inline]
    fn as_any(&self) -> &dyn std::any::Any { self }
}

impl Resource {
    fn new() -> Self {
        Self {
            addr_map: spin::Mutex::new(BTreeMap::new()),
        }
    }

    pub(crate) fn insert_addr_map(&self, local_addr: usize, remote_buf: ShmBuf) -> Result<(), ResourceError> {
        self.addr_map
            .lock()
            .insert(local_addr, remote_buf)
            .map_or_else(|| Ok(()), |_| Err(ResourceError::Exists))
    }

    #[inline]
    pub(crate) fn query_app_addr(&self, buf: ShmBuf) -> Result<usize, ResourceError> {
        let addr_map = self.addr_map.lock();
        match addr_map.range(0..=buf.ptr).last() {
            Some(kv) => {
                if kv.0 + kv.1.len >= buf.ptr + buf.len {
                    Ok(kv.1.ptr)
                } else {
                    Err(ResourceError::NotFound)
                }
            }
            None => Err(ResourceError::NotFound),
        }
    }
}
