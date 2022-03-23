//! Per-process state that is shared among multiple transport engines.
use fnv::FnvHashMap as HashMap;
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

use interface::{AsHandle, Handle};

use rdma::ibv;
use rdma::rdmacm;
use rdma::rdmacm::CmId;

use super::Error;
use crate::resource::ResourceTable;
use crate::state_mgr::{StateManager, StateTrait};

// TODO(cjr): Make this global lock more fine-grained.
pub(crate) struct State<'ctx> {
    sm: Arc<StateManager<Self>>,
    pub(crate) shared: Arc<Shared<'ctx>>,
}

impl<'ctx> Clone for State<'ctx> {
    fn clone(&self) -> Self {
        self.shared.alive_engines.fetch_add(1, Ordering::AcqRel);
        State {
            sm: Arc::clone(&self.sm),
            shared: Arc::clone(&self.shared),
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

impl<'ctx> StateTrait for State<'ctx> {
    type Err = io::Error;
    fn new(sm: Arc<StateManager<Self>>, pid: Pid) -> io::Result<Self> {
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
}

impl<'ctx> State<'ctx> {
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
        event_type: rdma::ffi::rdma_cm_event_type::Type,
    ) -> Option<rdmacm::CmEvent> {
        self.shared.cm_manager.try_lock().and_then(|mut manager| {
            // Get an event that match the req event type
            // If there's any event matches, return the first one,
            // otherwise, return None.
            let event_queue = manager
                .event_channel_buffer
                .entry(*event_channel_handle)
                .or_insert_with(VecDeque::default);

            if let Some(pos) = event_queue.iter().position(|e| e.event() == event_type) {
                event_queue.remove(pos)
            } else {
                None
            }
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
        if let Some(io_event) = events.iter().next() {
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
    pub(crate) pid: Pid,
    // Reference counting
    alive_engines: AtomicUsize,
    // Resources
    pub(crate) resource: Resource<'ctx>,
    // Other shared states include L4 policies, buffers, configurations, etc.
    _other: spin::Mutex<()>,
}

// TODO(cjr): move this to per-process state for better isolation
lazy_static! {
    pub(crate) static ref DEFAULT_CTXS: Vec<DefaultContext> =
        open_default_verbs().expect("Open default RDMA context failed.");
}

pub(crate) struct PinnedContext {
    pub(crate) verbs: ManuallyDrop<ibv::Context>,
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

pub(crate) struct DefaultContext {
    pub(crate) pinned_ctx: Pin<Box<PinnedContext>>,
    gid_table: Vec<ibv::Gid>,
}

/// Open default verbs contexts
fn open_default_verbs() -> io::Result<Vec<DefaultContext>> {
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
                default_ctxs.push(DefaultContext {
                    pinned_ctx: Box::pin(PinnedContext::new(ctx)),
                    gid_table,
                });
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
    pub(crate) default_pds: spin::Mutex<Vec<(interface::ProtectionDomain, Vec<ibv::Gid>)>>,
    // NOTE(cjr): Do NOT change the order of the following fields. A wrong drop order may cause
    // failures in the underlying library.
    pub(crate) cmid_table: ResourceTable<CmId<'ctx>>,
    pub(crate) event_channel_table: ResourceTable<rdmacm::EventChannel>,
    pub(crate) qp_table: ResourceTable<ibv::QueuePair<'ctx>>,
    pub(crate) mr_table: ResourceTable<rdma::mr::MemoryRegion>,
    pub(crate) cq_table: ResourceTable<ibv::CompletionQueue<'ctx>>,
    pub(crate) pd_table: ResourceTable<ibv::ProtectionDomain<'ctx>>,
}

impl<'ctx> Resource<'ctx> {
    pub(crate) fn new() -> io::Result<Self> {
        let mut default_pds = Vec::new();
        let pd_table = ResourceTable::default();
        for DefaultContext {
            pinned_ctx: ctx,
            gid_table,
        } in DEFAULT_CTXS.iter()
        {
            let pd = match ctx.verbs.alloc_pd() {
                Ok(pd) => pd,
                Err(_) => continue,
            };
            // TODO(cjr): Different resource of different devices can have the same handle
            // e.g. when SR-IOV is enabled, the program will panic here.
            let pd_handle = pd.as_handle();
            pd_table.insert(pd_handle, pd).unwrap();
            default_pds.push((interface::ProtectionDomain(pd_handle), gid_table.clone()));
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

    pub(crate) fn default_pd(&self, gid: &ibv::Gid) -> Option<interface::ProtectionDomain> {
        self.default_pds
            .lock()
            .iter()
            .find_map(|(pd, gids)| if gids.contains(&gid) { Some(*pd) } else { None })
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
        // This is safe because we did not drop these inner objects immediately. Instead, they are
        // stored carefully into the resource tables.
        let (pd, send_cq, recv_cq) = unsafe { qp.take_inner_objects() };
        let pd_handle = pd.as_handle();
        let scq_handle = send_cq.as_handle();
        let rcq_handle = recv_cq.as_handle();
        let qp_handle = qp.as_handle();
        // qp, cqs, and other associated resources are supposed to be open by the user
        // explicited, the error should be safely ignored if the resource has already been
        // created.
        self.qp_table.occupy_or_create_resource(qp_handle, qp);
        self.pd_table.occupy_or_create_resource(pd_handle, pd);
        self.cq_table.occupy_or_create_resource(scq_handle, send_cq);
        self.cq_table.occupy_or_create_resource(rcq_handle, recv_cq);
        Ok((qp_handle, pd_handle, scq_handle, rcq_handle))
    }

    pub(crate) fn insert_cmid(&self, cmid: CmId<'ctx>) -> Result<Handle, Error> {
        let cmid_handle = self.allocate_new_cmid_handle();
        self.cmid_table.insert(cmid_handle, cmid)?;
        Ok(cmid_handle)
    }
}
