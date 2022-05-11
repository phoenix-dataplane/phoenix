use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Once};

use fnv::FnvHashMap as HashMap;
use nix::unistd::Pid;

use interface::AsHandle;

use crate::mrpc::marshal::{SgList, ShmBuf};
use crate::resource::{Error as ResourceError, ResourceTable, ResourceTableGeneric};
use crate::rpc_adapter::ulib;
use crate::state_mgr::{StateManager, StateTrait};
use crate::transport::rdma::engine::TransportEngine;

pub(crate) struct State {
    sm: Arc<StateManager<Self>>,
    pub(crate) shared: Arc<Shared>,
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

impl Drop for State {
    fn drop(&mut self) {
        let was_last = self.shared.alive_engines.fetch_sub(1, Ordering::AcqRel) == 1;
        if was_last {
            let _ = self.sm.states.lock().remove(&self.shared.pid);
        }
    }
}

pub(crate) type CqBuffers =
    spin::Mutex<HashMap<interface::CompletionQueue, ulib::uverbs::CqBuffer>>;

pub(crate) struct Shared {
    pub(crate) pid: Pid,
    alive_engines: AtomicUsize,
    pub(crate) api_engine: spin::Mutex<TransportEngine>,
    pub(crate) resource: Resource,
    pub(crate) cq_buffers: CqBuffers,
}

#[derive(Debug)]
pub(crate) struct WrContext {
    pub(crate) conn_id: interface::Handle,
    pub(crate) mr_addr: usize,
}

#[derive(Debug)]
pub(crate) struct ReqContext {
    pub(crate) call_id: u64,
    pub(crate) sg_len: usize,
}

#[derive(Debug)]
pub(crate) struct ConnectionContext {
    pub(crate) cmid: ulib::ucm::CmId,
    pub(crate) credit: AtomicUsize,
    // call_id, sg_len
    pub(crate) outstanding_req: spin::Mutex<VecDeque<ReqContext>>,
    pub(crate) receiving_sgl: spin::Mutex<SgList>,
}

impl ConnectionContext {
    pub(crate) fn new(cmid: ulib::ucm::CmId, credit: usize) -> Self {
        Self {
            cmid,
            credit: AtomicUsize::new(credit),
            outstanding_req: spin::Mutex::new(VecDeque::new()),
            receiving_sgl: spin::Mutex::new(SgList(Vec::new())),
        }
    }
}

pub(crate) struct Resource {
    default_pd_flag: Once,
    default_pds: Vec<ulib::uverbs::ProtectionDomain>,
    pub(crate) mr_table: spin::Mutex<BTreeMap<usize, Arc<ulib::uverbs::MemoryRegion<u8>>>>,
    // TODO(cjr): we do not release any receive mr now. DO it in later version.
    pub(crate) recv_mr_table: ResourceTable<ulib::uverbs::MemoryRegion<u8>>,
    pub(crate) cmid_table: ResourceTable<ConnectionContext>,
    pub(crate) listener_table: ResourceTable<ulib::ucm::CmIdListener>,
    // wr_id -> WrContext
    pub(crate) wr_contexts: ResourceTableGeneric<u64, WrContext>,
}

impl Resource {
    fn new() -> Self {
        Self {
            default_pd_flag: Once::new(),
            default_pds: Vec::new(),
            mr_table: spin::Mutex::new(BTreeMap::default()),
            recv_mr_table: ResourceTable::default(),
            cmid_table: ResourceTable::default(),
            listener_table: ResourceTable::default(),
            wr_contexts: ResourceTableGeneric::default(),
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

    pub(crate) fn insert_mr(
        &self,
        mr: ulib::uverbs::MemoryRegion<u8>,
    ) -> Result<(), ResourceError> {
        self.mr_table
            .lock()
            .insert(mr.as_ptr() as usize, Arc::new(mr))
            .map_or_else(|| Ok(()), |_| Err(ResourceError::Exists))
    }

    pub(crate) fn query_mr(
        &self,
        sge: ShmBuf,
    ) -> Result<Arc<ulib::uverbs::MemoryRegion<u8>>, ResourceError> {
        let mr_table = self.mr_table.lock();
        match mr_table.range(0..=sge.ptr).last() {
            Some(kv) => {
                if kv.0 + kv.1.len() >= sge.ptr + sge.len {
                    Ok(Arc::clone(kv.1))
                } else {
                    Err(ResourceError::NotFound)
                }
            }
            None => Err(ResourceError::NotFound),
        }
    }
}

impl State {
    #[inline]
    pub(crate) fn resource(&self) -> &Resource {
        &self.shared.resource
    }
}
