use std::alloc::Layout;
use std::collections::BTreeMap;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use nix::unistd::Pid;

use interface::AsHandle;
use koala::engine::EngineLocalStorage;
use koala::resource::ResourceTable;
use koala::state_mgr::{StateManager, StateTrait};

use super::region::SharedRegion;
use super::{ControlPathError, ResourceError};

pub struct State {
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

pub struct Resource {
    // map from recv mr's local (backend) addr to app addr
    pub(crate) recv_mr_addr_map: spin::Mutex<BTreeMap<usize, mrpc_marshal::ShmRecvMr>>,
    // TODO(wyj): redesign these states
    pub(crate) recv_mr_table: ResourceTable<SharedRegion>,
    // TODO(wyj): apply the alignment trick and replace the BTreeMap here.
    // NOTE(wyj): removed Arc wrapper for SharedRegion, and accessing sender heap region is not needed
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
            recv_mr_addr_map: spin::Mutex::new(BTreeMap::new()),
            recv_mr_table: ResourceTable::default(),
            mr_table: spin::Mutex::new(BTreeMap::default()),
        }
    }

    pub(crate) fn allocate_recv_mr(
        &self,
        size: usize,
    ) -> Result<Arc<SharedRegion>, ControlPathError> {
        // TODO(cjr): update this
        let align = 8 * 1024 * 1024;
        let layout = Layout::from_size_align(size, align)?;
        let region = SharedRegion::new(layout)?;
        let handle = region.as_handle();
        self.recv_mr_table.insert(handle, region)?;
        let ret = self.recv_mr_table.get(&handle)?;
        Ok(ret)
    }

    pub(crate) fn insert_addr_map(
        &self,
        local_addr: usize,
        remote_buf: mrpc_marshal::ShmRecvMr,
    ) -> Result<(), ResourceError> {
        // SAFETY: it is the caller's responsibility to ensure the ShmMr is power-of-two aligned.

        // NOTE(wyj): local_addr is actually the pointer to the start of the recv_mr on backend side
        // the recv_mr on app side has the same length as the backend side
        // the length is logged in remote_buf
        self.recv_mr_addr_map
            .lock()
            .insert(local_addr, remote_buf)
            .map_or_else(|| Ok(()), |_| Err(ResourceError::Exists))
    }
}
