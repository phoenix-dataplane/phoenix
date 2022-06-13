use std::collections::VecDeque;
use std::io;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Once};

use dashmap::DashMap;
use fnv::FnvBuildHasher;
use nix::unistd::Pid;

use interface::AsHandle;

use crate::mrpc::marshal::SgList;
use crate::resource::{Error as ResourceError, ResourceTable, ResourceTableGeneric};
use crate::rpc_adapter::ulib;
use crate::state_mgr::{StateManager, StateTrait};

pub(crate) struct State {
    // per engine state
    pub(crate) rpc_adapter_id: usize,
    // shared among all engines of a user process
    sm: Arc<StateManager<Self>>,
    pub(crate) shared: Arc<Shared>,
    // per engine state
    cq: Option<ulib::uverbs::CompletionQueue>,
}

impl StateTrait for State {
    type Err = io::Error;
    fn new(sm: Arc<StateManager<Self>>, pid: Pid) -> Result<Self, Self::Err> {
        Ok(State {
            rpc_adapter_id: 0,
            sm,
            shared: Arc::new(Shared {
                pid,
                alive_engines: AtomicUsize::new(0),
                stop_acceptor: AtomicBool::new(false),
                resource: Resource::new(),
            }),
            cq: None,
        })
    }
}

impl Clone for State {
    fn clone(&self) -> Self {
        let rpc_adapter_id = self.shared.alive_engines.fetch_add(1, Ordering::AcqRel);
        State {
            rpc_adapter_id,
            sm: Arc::clone(&self.sm),
            shared: Arc::clone(&self.shared),
            cq: None,
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

pub(crate) struct Shared {
    pub(crate) pid: Pid,
    alive_engines: AtomicUsize,
    stop_acceptor: AtomicBool,
    resource: Resource,
}

#[derive(Debug)]
pub(crate) struct WrContext {
    pub(crate) conn_id: interface::Handle,
    pub(crate) mr_addr: usize,
}

#[derive(Debug)]
pub(crate) struct ReqContext {
    pub(crate) call_id: u32,
    pub(crate) sg_len: usize,
}

#[derive(Debug, Default)]
pub(crate) struct RecvContext {
    // buffer for recevied sges
    pub(crate) sg_list: SgList,
    // recv mrs that received sges are on  
    pub(crate) recv_mrs: Vec<interface::Handle>
}

#[derive(Debug)]
pub(crate) struct ConnectionContext {
    pub(crate) cmid: ulib::ucm::CmId,
    pub(crate) credit: AtomicUsize,
    // call_id, sg_len
    pub(crate) outstanding_req: spin::Mutex<VecDeque<ReqContext>>,
    pub(crate) receiving_ctx: spin::Mutex<RecvContext>,
}

impl ConnectionContext {
    pub(crate) fn new(cmid: ulib::ucm::CmId, credit: usize) -> Self {
        Self {
            cmid,
            credit: AtomicUsize::new(credit),
            outstanding_req: spin::Mutex::new(VecDeque::new()),
            receiving_ctx: spin::Mutex::new(RecvContext::default()),
        }
    }
}

pub(crate) struct Resource {
    default_pd_flag: Once,
    default_pds: Vec<ulib::uverbs::ProtectionDomain>,
    // TODO(cjr): we do not release any receive mr now. DO it in later version.
    // rpc_adapter_id -> Queue of pre_cmid
    pub(crate) pre_cmid_table: DashMap<usize, VecDeque<ulib::ucm::PreparedCmId>, FnvBuildHasher>,
    pub(crate) cmid_table: ResourceTable<ConnectionContext>,
    // (rpc_adapter_id, CmIdListener)
    pub(crate) listener_table: ResourceTable<(usize, ulib::ucm::CmIdListener)>,
    // wr_id -> WrContext
    pub(crate) wr_contexts: ResourceTableGeneric<u64, WrContext>,
    // CQ poll, for referencing cqs of other engines. The real CQ is owned by the
    // clone of the State of each engine.
    // rpc_adapter_id -> CQ
    pub(crate) cq_ref_table:
        ResourceTableGeneric<usize, ManuallyDrop<ulib::uverbs::CompletionQueue>>,
}

impl Resource {
    fn new() -> Self {
        Self {
            default_pd_flag: Once::new(),
            default_pds: Vec::new(),
            pre_cmid_table: DashMap::default(),
            cmid_table: ResourceTable::default(),
            listener_table: ResourceTable::default(),
            wr_contexts: ResourceTableGeneric::default(),
            cq_ref_table: ResourceTableGeneric::default(),
        }
    }

    #[inline]
    pub(crate) fn insert_cmid(
        &self,
        cmid: ulib::ucm::CmId,
        credit: usize,
    ) -> Result<(), ResourceError> {
        self.cmid_table
            .insert(cmid.as_handle(), ConnectionContext::new(cmid, credit))
    }

    pub(crate) fn default_pds(&self) -> &[ulib::uverbs::ProtectionDomain] {
        // safety: it is actually safe to mutate default_pds here, because it is only initialized
        // once in this call_once function.
        unsafe {
            self.default_pd_flag.call_once(|| {
                let ptr = &self.default_pds as *const Vec<_> as *mut Vec<_>;
                let default_pds = &mut *ptr;
                *default_pds = ulib::uverbs::get_default_pds().expect("Failed to get default PDs");
            });
            &self.default_pds
        }
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

    #[inline]
    pub(crate) fn acceptor_should_stop(&self) -> bool {
        self.shared.stop_acceptor.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn stop_acceptor(&self, stop: bool) {
        self.shared.stop_acceptor.store(stop, Ordering::Relaxed);
    }

    pub(crate) fn get_or_init_cq(&mut self) -> &ulib::uverbs::CompletionQueue {
        // this function is not supposed to be called concurrently.
        if self.cq.is_none() {
            // TODO(cjr): we currently by default use the first ibv_context.
            let ctx_list = ulib::uverbs::get_default_verbs_contexts().unwrap();
            let ctx = &ctx_list[0];
            self.cq = Some(ctx.create_cq(1024, 0).unwrap());
            let cq_handle = self.cq.as_ref().unwrap().as_handle();
            let cq_ref =
                ManuallyDrop::new(unsafe { ulib::uverbs::CompletionQueue::from_handle(cq_handle) });
            self.resource()
                .cq_ref_table
                .insert(self.rpc_adapter_id, cq_ref)
                .expect("cq in {self.rpc_adapter_id} already exists");
        }
        self.cq.as_ref().unwrap()
    }
}
