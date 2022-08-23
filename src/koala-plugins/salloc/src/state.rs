use std::alloc::Layout;
use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;

use interface::AsHandle;
use nix::unistd::Pid;

use crate::region::AddressMediator;

use super::region::SharedRegion;
use super::ControlPathError;
use koala::resource::{Error as ResourceError, ResourceTable};
use koala::state_mgr::ProcessShared;

pub struct State {
    pub(crate) shared: Arc<Shared>,
    pub(crate) addr_mediator: Arc<AddressMediator>,
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

    pub fn allocate_recv_mr(&self, size: usize) -> Result<Arc<SharedRegion>, ControlPathError> {
        // TODO(cjr): update this
        let align = 8 * 1024 * 1024;
        let layout = Layout::from_size_align(size, align)?;
        let region = SharedRegion::new(layout, &self.addr_mediator)?;
        let handle = region.as_handle();
        self.shared.resource.recv_mr_table.insert(handle, region)?;
        let ret = self.shared.resource.recv_mr_table.get(&handle)?;
        Ok(ret)
    }

    pub fn insert_addr_map(
        &self,
        local_addr: usize,
        remote_buf: mrpc_marshal::ShmRecvMr,
    ) -> Result<(), ResourceError> {
        // SAFETY: it is the caller's responsibility to ensure the ShmMr is power-of-two aligned.

        // NOTE(wyj): local_addr is actually the pointer to the start of the recv_mr on backend side
        // the recv_mr on app side has the same length as the backend side
        // the length is logged in remote_buf
        self.shared
            .resource
            .recv_mr_addr_map
            .lock()
            .insert(local_addr, remote_buf)
            .map_or_else(|| Ok(()), |_| Err(ResourceError::Exists))
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
    // map from recv mr's local (backend) addr to app addr
    pub recv_mr_addr_map: spin::Mutex<BTreeMap<usize, mrpc_marshal::ShmRecvMr>>,
    // TODO(wyj): redesign these states
    pub recv_mr_table: ResourceTable<SharedRegion>,
    // TODO(wyj): apply the alignment trick and replace the BTreeMap here.
    // NOTE(wyj): removed Arc wrapper for SharedRegion, and accessing sender heap region is not needed
    pub mr_table: spin::Mutex<BTreeMap<usize, SharedRegion>>,
}

impl Resource {
    fn new() -> Self {
        Self {
            recv_mr_addr_map: spin::Mutex::new(BTreeMap::new()),
            recv_mr_table: ResourceTable::default(),
            mr_table: spin::Mutex::new(BTreeMap::default()),
        }
    }
}
