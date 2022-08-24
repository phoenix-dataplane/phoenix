use std::collections::VecDeque;
use std::io;
use std::marker::PhantomPinned;
use std::mem::ManuallyDrop;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use lazy_static::lazy_static;
use nix::unistd::Pid;

use interface::{AsHandle, Handle};
use rdma::ibv;
use rdma::rdmacm;
use rdma::rdmacm::CmId;

use koala::resource::ResourceTable;
use koala::state_mgr::ProcessShared;
use koala::tracing;

use super::cm::CmEventManager;
use super::ApiError;

// TODO(cjr): Make this global lock more fine-grained.
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

pub struct Shared {
    // Control path operations must be per-process level
    // We use an async-friendly Mutex here
    pub(crate) cm_manager: tokio::sync::Mutex<CmEventManager>,
    // Pid as the identifier of this process
    pub pid: Pid,
    // Resources
    pub resource: Resource,
    // Other shared states include L4 policies, buffers, configurations, etc.
    _other: spin::Mutex<()>,
}

impl ProcessShared for Shared {
    type Err = io::Error;

    fn new(pid: Pid) -> io::Result<Self> {
        let cm_manager = tokio::sync::Mutex::new(CmEventManager::new()?);
        let shared = Shared {
            cm_manager,
            pid,
            resource: Resource::new()?,
            _other: spin::Mutex::new(()),
        };
        Ok(shared)
    }
}

// TODO(cjr): move this to per-process state for better isolation
// TODO(wyj): figure out whether this laay_static is going to be a issue
// when we move the engine to a shared library
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
                tracing::warn!("Skip device due to: {}", e);
                continue;
            }
        }
    }

    if default_ctxs.is_empty() {
        tracing::warn!("No active RDMA device found.");
    }
    Ok(default_ctxs)
}

#[derive(Debug)]
pub(crate) struct EventChannel {
    inner: rdmacm::EventChannel,
    event_queue: spin::Mutex<VecDeque<rdmacm::CmEvent>>,
}

impl AsHandle for EventChannel {
    #[inline]
    fn as_handle(&self) -> Handle {
        self.inner.as_handle()
    }
}

use std::ops::Deref;
impl Deref for EventChannel {
    type Target = rdmacm::EventChannel;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl EventChannel {
    pub(crate) fn new(inner: rdmacm::EventChannel) -> Self {
        Self {
            inner,
            event_queue: spin::Mutex::new(VecDeque::new()),
        }
    }

    /// Get an event that matches the event type.
    ///
    /// If there's any event matches, return the first one matched. Otherwise, returns None.
    pub(crate) fn get_one_cm_event(
        &self,
        event_type: rdma::ffi::rdma_cm_event_type::Type,
    ) -> Option<rdmacm::CmEvent> {
        let mut event_queue = self.event_queue.lock();
        if let Some(pos) = event_queue.iter().position(|e| e.event() == event_type) {
            event_queue.remove(pos)
        } else {
            None
        }
    }

    pub(crate) fn add_event(&self, cm_event: rdmacm::CmEvent) {
        self.event_queue.lock().push_back(cm_event);
    }

    pub(crate) fn clear_event_queue(&self) {
        self.event_queue.lock().clear();
    }
}

/// A variety of tables where each maps a `Handle` to a kind of RNIC resource.
pub struct Resource {
    cmid_cnt: AtomicU32,
    pub default_pds: spin::Mutex<Vec<(interface::ProtectionDomain, Vec<ibv::Gid>)>>,
    // NOTE(cjr): Do NOT change the order of the following fields. A wrong drop order may cause
    // failures in the underlying library.
    pub cmid_table: ResourceTable<CmId<'static>>,
    // NOTE(cjr): On drop, the events in EventChannel buffer must be dropped first before dropping CmId
    pub event_channel_table: ResourceTable<EventChannel>,
    pub qp_table: ResourceTable<ibv::QueuePair<'static>>,
    pub mr_table: ResourceTable<rdma::mr::MemoryRegion>,
    pub cq_table: ResourceTable<ibv::CompletionQueue<'static>>,
    pub pd_table: ResourceTable<ibv::ProtectionDomain<'static>>,
}

/// __Safety__: This is safe because only default_pds is non-concurrent-safe, but it is only
/// accessed when creating the resource.
unsafe impl Send for Resource {}
unsafe impl Sync for Resource {}

impl Resource {
    pub fn new() -> io::Result<Self> {
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

    pub fn default_pd(&self, gid: &ibv::Gid) -> Option<interface::ProtectionDomain> {
        self.default_pds
            .lock()
            .iter()
            .find_map(|(pd, gids)| if gids.contains(&gid) { Some(*pd) } else { None })
    }

    pub fn allocate_new_cmid_handle(&self) -> Handle {
        let ret = Handle(self.cmid_cnt.load(Ordering::Acquire));
        self.cmid_cnt.fetch_add(1, Ordering::AcqRel);
        ret
    }

    pub fn insert_qp(
        &self,
        qp: ibv::QueuePair<'static>,
    ) -> Result<(Handle, Handle, Handle, Handle), ApiError> {
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

    pub fn insert_cmid(&self, cmid: CmId<'static>) -> Result<Handle, ApiError> {
        let cmid_handle = self.allocate_new_cmid_handle();
        self.cmid_table.insert(cmid_handle, cmid)?;
        Ok(cmid_handle)
    }
}
