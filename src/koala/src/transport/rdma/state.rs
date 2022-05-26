//! Per-process state that is shared among multiple transport engines.
use std::io;
use std::marker::PhantomPinned;
use std::mem::ManuallyDrop;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;

use lazy_static::lazy_static;
use nix::unistd::Pid;

use interface::{AsHandle, Handle};

use rdma::ibv;
use rdma::rdmacm;
use rdma::rdmacm::CmId;

use super::cm::CmEventManager;
use super::ApiError;
use crate::resource::ResourceTable;
use crate::state_mgr::{StateManager, StateTrait};

// TODO(cjr): Make this global lock more fine-grained.
pub(crate) struct State {
    sm: Arc<StateManager<Self>>,
    pub(crate) shared: Arc<Shared>,
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

impl StateTrait for State {
    type Err = io::Error;
    fn new(sm: Arc<StateManager<Self>>, pid: Pid) -> io::Result<Self> {
        Ok(State {
            sm,
            shared: Arc::new(Shared {
                cm_manager: tokio::sync::Mutex::new(CmEventManager::new()?),
                pid,
                alive_engines: AtomicUsize::new(0),
                resource: Resource::new()?,
                _other: spin::Mutex::new(()),
            }),
        })
    }
}

impl State {
    #[inline]
    pub(crate) fn resource(&self) -> &Resource {
        &self.shared.resource
    }

    #[inline]
    pub(crate) fn alive_engines(&self) -> usize {
        self.shared.alive_engines.load(Ordering::Relaxed)
    }
}

pub(crate) struct Shared {
    // Control path operations must be per-process level
    // We use an async-friendly Mutex here
    pub(crate) cm_manager: tokio::sync::Mutex<CmEventManager>,
    // Pid as the identifier of this process
    pub(crate) pid: Pid,
    // Reference counting
    alive_engines: AtomicUsize,
    // Resources
    pub(crate) resource: Resource,
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
unsafe impl Send for Resource {}
unsafe impl Sync for Resource {}

/// A variety of tables where each maps a `Handle` to a kind of RNIC resource.
pub(crate) struct Resource {
    cmid_cnt: AtomicU32,
    pub(crate) default_pds: spin::Mutex<Vec<(interface::ProtectionDomain, Vec<ibv::Gid>)>>,
    // NOTE(cjr): Do NOT change the order of the following fields. A wrong drop order may cause
    // failures in the underlying library.
    pub(crate) cmid_table: ResourceTable<CmId<'static>>,
    pub(crate) event_channel_table: ResourceTable<rdmacm::EventChannel>,
    pub(crate) qp_table: ResourceTable<ibv::QueuePair<'static>>,
    pub(crate) mr_table: ResourceTable<rdma::mr::MemoryRegion>,
    pub(crate) cq_table: ResourceTable<ibv::CompletionQueue<'static>>,
    pub(crate) pd_table: ResourceTable<ibv::ProtectionDomain<'static>>,
}

impl Resource {
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

    pub(crate) fn insert_cmid(&self, cmid: CmId<'static>) -> Result<Handle, ApiError> {
        let cmid_handle = self.allocate_new_cmid_handle();
        self.cmid_table.insert(cmid_handle, cmid)?;
        Ok(cmid_handle)
    }
}
