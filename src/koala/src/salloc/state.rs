use std::alloc::Layout;
use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;

use interface::AsHandle;
use nix::unistd::Pid;

use super::region::SharedRegion;
use super::ControlPathError;
use crate::engine::EngineLocalStorage;
use crate::resource::{Error as ResourceError, ResourceTable};
use crate::state_mgr::ProcessShared;

pub(crate) struct State {
    pub(crate) shared: Arc<Shared>,
}

impl State {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        State { shared }
    }
}

impl State {
    #[inline]
    pub(crate) fn resource(&self) -> &Resource {
        &self.shared.resource
    }
}

pub(crate) struct Shared {
    pub(crate) pid: Pid,
    pub(crate) resource: Resource,
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

pub(crate) struct Resource {
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
