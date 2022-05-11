use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nix::unistd::Pid;

use crate::engine::EngineLocalStorage;
use crate::resource::Error as ResourceError;
use crate::state_mgr::{StateManager, StateTrait};

pub(crate) struct ShmMr {
    pub ptr: usize,
    pub len: usize,
    pub align: usize,
}

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
    // map from recv mr's local (backend) addr to app addr
    pub(crate) recv_mr_addr_map: spin::Mutex<BTreeMap<usize, ShmMr>>,
}

unsafe impl EngineLocalStorage for Resource {
    #[inline]
    fn as_any(&self) -> &dyn std::any::Any { self }
}

impl Resource {
    fn new() -> Self {
        Self {
            recv_mr_addr_map: spin::Mutex::new(BTreeMap::new()),
        }
    }

    pub(crate) fn insert_addr_map(&self, local_addr: usize, remote_buf: ShmMr) -> Result<(), ResourceError> {
        // SAFETY: it is the caller's responsibility to ensure the ShmMr is power-of-two aligned.

        // NOTE(wyj): local_addr is actually the pointer to the start of the recv_mr on backend side
        // the recv_mr on app side has the same length as the backend side
        // the length is logged in remote_buf
        self.recv_mr_addr_map
            .lock()
            .insert(local_addr, remote_buf)
            .map_or_else(|| Ok(()), |_| Err(ResourceError::Exists))
    
    }
    #[inline]
    pub(crate) fn query_app_addr(&self, local_addr: usize) -> Result<usize, ResourceError> {
        // local_addr is the address on the backend side
        let addr_map = self.recv_mr_addr_map.lock();
        match addr_map.range(0..=local_addr).last() {
            Some(kv) => {
                if kv.0 + kv.1.len >= local_addr {
                    let offset = local_addr;
                    Ok(kv.1.ptr + offset)
                } else {
                    Err(ResourceError::NotFound)
                }
            }
            None => Err(ResourceError::NotFound),
        }
    }
}
