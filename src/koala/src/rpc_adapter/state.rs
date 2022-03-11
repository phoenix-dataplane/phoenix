use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use fnv::FnvHashMap as HashMap;
use nix::unistd::Pid;

use interface::AsHandle;

use crate::resource::{ResourceTable, Error as ResourceError};
use crate::rpc_adapter::ulib;
use crate::state_mgr::{StateManager, StateTrait};

pub(crate) struct State {
    sm: Arc<StateManager<Self>>,
    pub(crate) shared: Arc<Shared>,
}

pub(crate) type CqBuffers =
    spin::Mutex<HashMap<interface::CompletionQueue, ulib::uverbs::CqBuffer>>;

pub(crate) struct Shared {
    pid: Pid,
    alive_engines: AtomicUsize,
    resource: Resource,
    pub(crate) cq_buffers: CqBuffers,
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
                cq_buffers: spin::Mutex::new(HashMap::default()),
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

pub(crate) struct Resource {
    cmid_table: ResourceTable<ulib::ucm::CmId>,
}

impl Resource {
    fn new() -> Self {
        Self {
            cmid_table: ResourceTable::default(),
        }
    }

    #[inline]
    pub(crate) fn insert_cmid(&self, cmid: ulib::ucm::CmId) -> Result<(), ResourceError> {
        self.cmid_table.insert(cmid.as_handle(), cmid)
    }
}

impl State {
    #[inline]
    pub(crate) fn resource(&self) -> &Resource {
        &self.shared.resource
    }
}
