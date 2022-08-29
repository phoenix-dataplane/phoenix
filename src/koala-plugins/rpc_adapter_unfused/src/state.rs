use std::collections::VecDeque;
use std::io;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering, AtomicU64};
use std::sync::Arc;

use dashmap::DashMap;
use fnv::FnvBuildHasher;
use nix::unistd::Pid;

use interface::AsHandle;
use mrpc_marshal::SgList;

use salloc::region::AddressMediator;

use koala::resource::{Error as ResourceError, ResourceTable, ResourceTableGeneric};
use koala::state_mgr::ProcessShared;

use super::pool::{BufferPool, RecvBuffer};
use super::serialization::AddressMap;
use super::ulib;

pub(crate) struct State {
    // per engine state
    pub(crate) rpc_adapter_id: usize,
    pub(crate) shared: Arc<Shared>,
    // per engine state
    cq: Option<ulib::uverbs::CompletionQueue>,
}

impl State {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        // Arc's refcnt should be the number of RpcAdapter engines
        // serving the user application process
        // including the current one
        // as State is only attached to RpcAdapter
        let rpc_adapter_id = Arc::strong_count(&shared) - 1;
        State {
            rpc_adapter_id,
            shared,
            cq: None,
        }
    }
}

pub struct Shared {
    pub pid: Pid,
    stop_acceptor: AtomicBool,
    pub resource: Resource,
}

impl ProcessShared for Shared {
    type Err = io::Error;

    fn new(_pid: Pid) -> io::Result<Self> {
        panic!("should not use this function")
    }
}

impl Shared {
    pub(crate) fn new_from_addr_mediator(
        pid: Pid,
        addr_mediator: Arc<AddressMediator>,
    ) -> io::Result<Self> {
        let shared = Shared {
            pid,
            stop_acceptor: AtomicBool::new(false),
            resource: Resource::new(addr_mediator),
        };
        Ok(shared)
    }
}

#[derive(Debug)]
pub(crate) struct WrContext {
    pub(crate) conn_id: interface::Handle,
    pub(crate) buffer_addr: usize,
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
    pub(crate) recv_buffer_handles: Vec<interface::Handle>,
}

#[derive(Debug)]
pub(crate) struct ConnectionContext {
    pub(crate) cmid: ulib::ucm::CmId,
    pub(crate) credit: AtomicUsize,
    // remote end's RpcAdapter engine (major) version
    // we assume there is no breaking change that requires
    // both endpoints' engines need to be upgraded,
    // if major version is not changed
    // initalized to 0, indicate no version has been received
    pub(crate) remote_version: AtomicU64,
    // local version
    pub(crate) local_version: AtomicU64,
    // call_id, sg_len
    pub(crate) outstanding_req: spin::Mutex<VecDeque<ReqContext>>,
    pub(crate) receiving_ctx: spin::Mutex<RecvContext>,
}

impl ConnectionContext {
    pub(crate) fn new(cmid: ulib::ucm::CmId, credit: usize, local_version: u64) -> Self {
        Self {
            cmid,
            credit: AtomicUsize::new(credit),
            remote_version: AtomicU64::new(0),
            local_version: AtomicU64::new(local_version),
            outstanding_req: spin::Mutex::new(VecDeque::new()),
            receiving_ctx: spin::Mutex::new(RecvContext::default()),
        }
    }
}

// NOTE: Pay attention to the drop order.
pub struct Resource {
    // rpc_adapter_id -> Queue of pre_cmid
    pub(crate) pre_cmid_table: DashMap<usize, VecDeque<ulib::ucm::PreparedCmId>, FnvBuildHasher>,
    pub(crate) staging_pre_cmid_table: ResourceTable<ulib::ucm::PreparedCmId>,
    pub(crate) cmid_table: ResourceTable<ConnectionContext>,
    // (rpc_adapter_id, CmIdListener)
    pub(crate) listener_table: ResourceTable<(usize, ulib::ucm::CmIdListener)>,
    // wr_id -> WrContext
    pub(crate) wr_contexts: ResourceTableGeneric<u64, WrContext>,

    // map from recv buffer's local addr (backend) to app addr (frontend)
    pub(crate) addr_map: AddressMap,
    // TODO(wyj): redesign these states
    pub(crate) recv_buffer_table: ResourceTable<RecvBuffer>,
    // receive buffer pool
    pub(crate) recv_buffer_pool: BufferPool,

    // CQ poll, for referencing cqs of other engines. The real CQ is owned by the
    // clone of the State of each engine.
    // rpc_adapter_id -> CQ
    pub(crate) cq_ref_table:
        ResourceTableGeneric<usize, ManuallyDrop<ulib::uverbs::CompletionQueue>>,
}

impl Resource {
    fn new(addr_mediator: Arc<AddressMediator>) -> Self {
        Self {
            pre_cmid_table: DashMap::default(),
            staging_pre_cmid_table: ResourceTable::default(),
            cmid_table: ResourceTable::default(),
            listener_table: ResourceTable::default(),
            wr_contexts: ResourceTableGeneric::default(),
            addr_map: AddressMap::new(),
            recv_buffer_table: ResourceTable::default(),
            recv_buffer_pool: BufferPool::new(addr_mediator),
            cq_ref_table: ResourceTableGeneric::default(),
        }
    }

    #[inline]
    pub(crate) fn insert_cmid(
        &self,
        cmid: ulib::ucm::CmId,
        credit: usize,
        local_version: u64,
    ) -> Result<(), ResourceError> {
        self.cmid_table
            .insert(cmid.as_handle(), ConnectionContext::new(cmid, credit, local_version))
    }
}

impl State {
    #[inline]
    pub(crate) fn resource(&self) -> &Resource {
        &self.shared.resource
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
