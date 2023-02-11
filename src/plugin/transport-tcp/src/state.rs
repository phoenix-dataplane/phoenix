//! Per-process state that is shared among multiple transport engines.
use std::cell::RefCell;
use std::io;
use std::sync::Arc;

use fnv::FnvHashMap as HashMap;
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Poll};
use nix::unistd::Pid;

use phoenix_common::state_mgr::ProcessShared;
use uapi::net::MappedAddrStatus;
use uapi::Handle;

use super::ops::CompletionQueue;

// TODO(cjr): Make this global lock more fine-grained.
pub struct State {
    pub(crate) shared: Arc<Shared>,
    pub poll: RefCell<Poll>,
    pub events: RefCell<Events>,
    pub listener_table: RefCell<HashMap<Handle, TcpListener>>,
    pub sock_table: RefCell<HashMap<Handle, (TcpStream, MappedAddrStatus)>>,
    // conn_handle -> completion queue
    pub cq_table: RefCell<HashMap<Handle, CompletionQueue>>,
}

// SAFETY: State in tcp will not be shared by multiple threads
// It is owned and used by a single thread/runtime
unsafe impl Sync for State {}

impl State {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        State {
            shared,
            poll: RefCell::new(Poll::new().unwrap()),
            events: RefCell::new(Events::with_capacity(1024)),
            listener_table: RefCell::new(HashMap::default()),
            sock_table: RefCell::new(HashMap::default()),
            cq_table: RefCell::new(HashMap::default()),
        }
    }
}

impl Clone for State {
    fn clone(&self) -> Self {
        State {
            shared: Arc::clone(&self.shared),
            poll: RefCell::new(Poll::new().unwrap()),
            events: RefCell::new(Events::with_capacity(1024)),
            listener_table: RefCell::new(HashMap::default()),
            sock_table: RefCell::new(HashMap::default()),
            cq_table: RefCell::new(HashMap::default()),
        }
    }
}

impl State {
    #[inline]
    pub(crate) fn _resource(&self) -> &Resource {
        &self.shared._resource
    }
}

pub struct Shared {
    // Control path operations must be per-process level

    // Pid as the identifier of this process
    pub(crate) pid: Pid,
    // Resources
    pub(crate) _resource: Resource,
    // Other shared states include L4 policies, buffers, configurations, etc.
    _other: spin::Mutex<()>,
}

impl ProcessShared for Shared {
    type Err = io::Error;

    fn new(pid: Pid) -> io::Result<Self> {
        let shared = Shared {
            pid,
            _resource: Resource::new()?,
            _other: spin::Mutex::new(()),
        };
        Ok(shared)
    }
}

pub(crate) struct Resource {}

/// __Safety__: This is safe because only default_pds is non-concurrent-safe, but it is only
/// accessed when creating the resource.
unsafe impl Send for Resource {}
unsafe impl Sync for Resource {}

impl Resource {
    pub(crate) fn new() -> io::Result<Self> {
        Ok(Resource {})
    }
}
