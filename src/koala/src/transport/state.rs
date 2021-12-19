//! Per-process state that is shared among multiple transport engines.
use fnv::FnvHashMap as HashMap;
use std::collections::hash_map;
use std::collections::VecDeque;
use std::io;
use std::marker::PhantomPinned;
use std::mem::ManuallyDrop;
use std::os::unix::io::AsRawFd;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use lazy_static::lazy_static;
use nix::unistd::Pid;

use interface::Handle;

use rdma::ibv;
use rdma::rdmacm;
use rdma::rdmacm::CmId;

use super::resource::ResourceTable;
use super::Error;

// Per-process state
pub(crate) struct StateManager<'ctx> {
    states: spin::Mutex<HashMap<Pid, State<'ctx>>>,
}

impl<'ctx> StateManager<'ctx> {
    pub(crate) fn new() -> Self {
        StateManager {
            states: spin::Mutex::new(HashMap::default()),
        }
    }

    pub(crate) fn get_or_create_state(&'ctx self, pid: Pid) -> io::Result<State<'ctx>> {
        let mut states = self.states.lock();
        match states.entry(pid) {
            hash_map::Entry::Occupied(e) => {
                // refcount ++
                Ok(e.get().clone())
            }
            hash_map::Entry::Vacant(e) => {
                let state = State::new(&self, pid)?;
                // refcount ++
                let ret = state.clone();
                e.insert(state);
                Ok(ret)
            }
        }
    }
}

// TODO(cjr): Make this global lock more fine-grained.
pub(crate) struct State<'ctx> {
    sm: &'ctx StateManager<'ctx>,
    pub(crate) shared: Arc<Shared<'ctx>>,
}

impl<'ctx> Clone for State<'ctx> {
    fn clone(&self) -> Self {
        self.shared.alive_engines.fetch_add(1, Ordering::AcqRel);
        State {
            shared: Arc::clone(&self.shared),
            sm: self.sm,
        }
    }
}

impl<'ctx> Drop for State<'ctx> {
    fn drop(&mut self) {
        let was_last = self.shared.alive_engines.fetch_sub(1, Ordering::AcqRel) == 1;
        if was_last {
            let _ = self.sm.states.lock().remove(&self.shared.pid);
        }
    }
}

impl<'ctx> State<'ctx> {
    pub(crate) fn new(sm: &'ctx StateManager<'ctx>, pid: Pid) -> io::Result<Self> {
        Ok(State {
            sm,
            shared: Arc::new(Shared {
                cm_manager: spin::Mutex::new(CmEventManager::new()?),
                pid,
                alive_engines: AtomicUsize::new(0),
                resource: Resource::new()?,
                _other: spin::Mutex::new(()),
            }),
        })
    }

    #[inline]
    pub(crate) fn resource(&self) -> &Resource<'ctx> {
        &self.shared.resource
    }

    pub(crate) fn register_event_channel(
        &self,
        channel_handle: Handle,
        channel: &rdmacm::EventChannel,
    ) -> Result<(), Error> {
        self.shared
            .cm_manager
            .lock()
            .poll
            .registry()
            .register(
                &mut mio::unix::SourceFd(&channel.as_raw_fd()),
                mio::Token(channel_handle.0 as _),
                mio::Interest::READABLE,
            )
            .map_err(Error::Mio)?;
        Ok(())
    }

    pub(crate) fn poll_cm_event_once(&self) -> Result<(), Error> {
        self.shared
            .cm_manager
            .try_lock()
            .map_or(Ok(()), |mut manager| {
                manager.poll_cm_event_once(&self.resource().event_channel_table)
            })
    }

    pub(crate) fn get_one_cm_event(
        &self,
        event_channel_handle: &Handle,
    ) -> Option<rdmacm::CmEvent> {
        self.shared.cm_manager.try_lock().and_then(|mut manager| {
            manager
                .event_channel_buffer
                .entry(*event_channel_handle)
                .or_insert_with(VecDeque::default)
                .pop_front()
        })
    }
}

struct CmEventManager {
    poll: mio::Poll,
    event_channel_buffer: HashMap<interface::Handle, VecDeque<rdmacm::CmEvent>>,
}

impl CmEventManager {
    fn new() -> io::Result<Self> {
        Ok(CmEventManager {
            poll: mio::Poll::new()?,
            event_channel_buffer: HashMap::default(),
        })
    }

    fn poll_cm_event_once(
        &mut self,
        event_channel_table: &ResourceTable<rdmacm::EventChannel>,
    ) -> Result<(), Error> {
        let mut events = mio::Events::with_capacity(1);
        self.poll
            .poll(&mut events, Some(Duration::from_millis(1)))
            .map_err(Error::Mio)?;
        for io_event in &events {
            let handle = Handle(io_event.token().0 as _);
            let event_channel = event_channel_table.get(&handle)?;
            // read one event
            let cm_event = event_channel.get_cm_event().map_err(Error::RdmaCm)?;
            // reregister everytime to simulate level-trigger
            self.poll
                .registry()
                .reregister(
                    &mut mio::unix::SourceFd(&event_channel.as_raw_fd()),
                    io_event.token(),
                    mio::Interest::READABLE,
                )
                .map_err(Error::Mio)?;
            // append to the local buffer
            self.event_channel_buffer
                .entry(handle)
                .or_insert_with(VecDeque::default)
                .push_back(cm_event);
            return Ok(());
        }
        Err(Error::NoCmEvent)
    }
}

pub(crate) struct Shared<'ctx> {
    // Control path operations must be per-process level
    cm_manager: spin::Mutex<CmEventManager>,
    // We use pid as the identifier of this process
    pid: Pid,
    // Reference counting
    alive_engines: AtomicUsize,
    // Resources
    pub(crate) resource: Resource<'ctx>,
    // Other shared states include L4 policies, buffers, configurations, etc.
    _other: spin::Mutex<()>,
}

// TODO(cjr): move this to per-process state for better isolation
lazy_static! {
    static ref DEFAULT_CTXS: Vec<(Pin<Box<PinnedContext>>, Vec<ibv::Gid>)> =
        open_default_verbs().expect("Open default RDMA context failed.");
}

struct PinnedContext {
    verbs: ManuallyDrop<ibv::Context>,
    _pin: PhantomPinned,
}

impl PinnedContext {
    fn new(ctx: ManuallyDrop<ibv::Context>) -> Self {
        PinnedContext {
            verbs: ctx,
            _pin: PhantomPinned,
        }
    }
}

/// Open default verbs contexts
fn open_default_verbs() -> io::Result<Vec<(Pin<Box<PinnedContext>>, Vec<ibv::Gid>)>> {
    let mut default_ctxs = Vec::new();
    let ctx_list = rdmacm::get_devices()?;
    for ctx in ctx_list.into_iter() {
        let result: io::Result<_> = (|| {
            let max_index = ctx.port_attr()?.gid_tbl_len as usize;
            let gid_table: io::Result<_> = (0..max_index).map(|index| ctx.gid(index)).collect();
            Ok((ctx, gid_table?))
        })();
        match result {
            Ok((ctx, gid_table)) => {
                default_ctxs.push((Box::pin(PinnedContext::new(ctx)), gid_table));
            }
            Err(e) => {
                warn!("Skip device due to: {}", e);
                continue;
            }
        }
    }

    if default_ctxs.is_empty() {
        warn!("No active RDMA device found.");
    }
    Ok(default_ctxs)
}

/// __Safety__: This is safe because only default_pds is non-concurrent-safe, but it is only
/// accessed when creating the resource.
unsafe impl<'ctx> Send for Resource<'ctx> {}
unsafe impl<'ctx> Sync for Resource<'ctx> {}

/// A variety of tables where each maps a `Handle` to a kind of RNIC resource.
pub(crate) struct Resource<'ctx> {
    cmid_cnt: AtomicU32,
    default_pds: spin::Mutex<HashMap<ibv::Gid, interface::ProtectionDomain>>,
    // NOTE(cjr): Do NOT change the order of the following fields. A wrong drop order may cause
    // failures in the underlying library.
    pub(crate) cmid_table: ResourceTable<CmId>,
    pub(crate) event_channel_table: ResourceTable<rdmacm::EventChannel>,
    pub(crate) qp_table: ResourceTable<ibv::QueuePair<'ctx>>,
    pub(crate) mr_table: ResourceTable<rdma::mr::MemoryRegion>,
    pub(crate) cq_table: ResourceTable<ibv::CompletionQueue<'ctx>>,
    pub(crate) pd_table: ResourceTable<ibv::ProtectionDomain<'ctx>>,
}

impl<'ctx> Resource<'ctx> {
    pub(crate) fn new() -> io::Result<Self> {
        let mut default_pds = HashMap::default();
        let pd_table = ResourceTable::default();
        for (ctx, gid_table) in DEFAULT_CTXS.iter() {
            let pd = match ctx.verbs.alloc_pd() {
                Ok(pd) => pd,
                Err(_) => continue,
            };
            let pd_handle = pd.handle().into();
            pd_table.insert(pd_handle, pd).unwrap();
            for gid in gid_table {
                default_pds.insert(*gid, interface::ProtectionDomain(pd_handle));
            }
        }
        Ok(Resource {
            cmid_cnt: AtomicU32::new(0),
            default_pds: spin::Mutex::new(default_pds),
            cmid_table: ResourceTable::default(),
            event_channel_table: ResourceTable::default(),
            qp_table: ResourceTable::default(),
            mr_table: ResourceTable::default(),
            cq_table: ResourceTable::default(),
            pd_table,
        })
    }

    pub(crate) fn default_pd(&self, gid: &ibv::Gid) -> interface::ProtectionDomain {
        self.default_pds.lock()[gid]
    }

    pub(crate) fn allocate_new_cmid_handle(&self) -> Handle {
        let ret = Handle(self.cmid_cnt.load(Ordering::Acquire));
        self.cmid_cnt.fetch_add(1, Ordering::AcqRel);
        ret
    }

    pub(crate) fn insert_qp(
        &self,
        qp: ibv::QueuePair<'ctx>,
    ) -> Result<(Handle, Handle, Handle, Handle), Error> {
        let pd = qp.pd();
        let send_cq = qp.send_cq();
        let recv_cq = qp.recv_cq();
        let pd_handle = pd.handle().into();
        let scq_handle = send_cq.handle().into();
        let rcq_handle = recv_cq.handle().into();
        let qp_handle = qp.handle().into();
        // qp, cqs, and other associated resources are supposed to be open by the user
        // explicited, the error should be safely ignored if the resource has already been
        // created.
        self.qp_table.occupy_or_create_resource(qp_handle, qp);
        self.pd_table.occupy_or_create_resource(pd_handle, pd);
        self.cq_table.occupy_or_create_resource(scq_handle, send_cq);
        self.cq_table.occupy_or_create_resource(rcq_handle, recv_cq);
        Ok((qp_handle, pd_handle, scq_handle, rcq_handle))
    }

    pub(crate) fn insert_cmid(
        &self,
        cmid: CmId,
    ) -> Result<(Handle, Option<(Handle, Handle, Handle, Handle)>), Error> {
        let cmid_handle = self.allocate_new_cmid_handle();
        if let Some(qp) = cmid.qp() {
            self.cmid_table.insert(cmid_handle, cmid)?;
            let handles = Some(self.insert_qp(qp)?);
            Ok((cmid_handle, handles))
        } else {
            // listener cmid, no QP associated.
            self.cmid_table.insert(cmid_handle, cmid)?;
            Ok((cmid_handle, None))
        }
    }
}
