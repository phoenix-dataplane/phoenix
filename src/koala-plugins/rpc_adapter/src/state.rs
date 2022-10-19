use std::collections::VecDeque;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use dashmap::DashMap;
use fnv::FnvBuildHasher;
use nix::unistd::Pid;

use interface::AsHandle;
use interface::rpc::CallId;
use mrpc_marshal::SgList;

use salloc::region::AddressMediator;

use koala::local_resource::{LocalResourceTable, LocalResourceTableGeneric};
use koala::resource::{Error as ResourceError, ResourceTable};
use koala::state_mgr::ProcessShared;

use super::pool::{BufferPool, RecvBuffer};
use super::serialization::AddressMap;
use super::ulib;

// TODO(cjr): Currently we do not have concurrent access to State while upgrading. But we need to
// be careful when this assumption does not hold in the future.
unsafe impl Sync for State {}

pub(crate) struct State {
    // per engine state
    pub(crate) rpc_adapter_id: usize,
    pub(crate) shared: Arc<Shared>,
    // per engine state
    local_resource: LocalResource,
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
            local_resource: LocalResource::new(),
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
    pub(crate) call_id: CallId,
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

pub struct LocalResource {
    pub(crate) cmid_table: LocalResourceTable<ConnectionContext>,
    // wr_id -> WrContext
    pub(crate) wr_contexts: LocalResourceTableGeneric<u64, WrContext>,
    // TODO(wyj): redesign these states
    pub(crate) recv_buffer_table: LocalResourceTable<RecvBuffer>,
    // map from recv buffer's local addr (backend) to app addr (frontend)
    pub(crate) addr_map: AddressMap,
    // Per-thread CQ
    pub(crate) cq: Option<ulib::uverbs::CompletionQueue>,
}

impl LocalResource {
    fn new() -> Self {
        Self {
            cmid_table: LocalResourceTable::default(),
            wr_contexts: LocalResourceTableGeneric::default(),
            recv_buffer_table: LocalResourceTable::default(),
            addr_map: AddressMap::new(),
            cq: None,
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
}

// NOTE: Pay attention to the drop order.
pub struct Resource {
    // rpc_adapter_id -> Queue of pre_cmid
    pub(crate) builder_table: DashMap<
        usize,
        VecDeque<ulib::ucm::CmIdBuilder<'static, 'static, 'static, 'static, 'static>>,
        FnvBuildHasher,
    >,
    pub(crate) staging_pre_cmid_table: ResourceTable<ulib::ucm::PreparedCmId>,
    // (rpc_adapter_id, CmIdListener)
    pub(crate) listener_table: ResourceTable<(usize, ulib::ucm::CmIdListener)>,

    // receive buffer pool
    pub(crate) recv_buffer_pool: BufferPool,
}

impl Resource {
    fn new(addr_mediator: Arc<AddressMediator>) -> Self {
        Self {
            builder_table: DashMap::default(),
            staging_pre_cmid_table: ResourceTable::default(),
            listener_table: ResourceTable::default(),
            recv_buffer_pool: BufferPool::new(addr_mediator),
        }
    }
}

impl State {
    #[inline]
    pub(crate) fn resource(&self) -> &Resource {
        &self.shared.resource
    }

    #[inline]
    pub(crate) fn local_resource(&self) -> &LocalResource {
        &self.local_resource
    }

    #[inline]
    pub(crate) fn acceptor_should_stop(&self) -> bool {
        self.shared.stop_acceptor.load(Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn stop_acceptor(&self, stop: bool) {
        self.shared.stop_acceptor.store(stop, Ordering::Relaxed);
    }

    pub(crate) fn get_or_init_cq(
        &mut self,
        cq_size: i32,
        cq_context: u64,
        builder: &ulib::ucm::CmIdBuilder,
    ) -> Result<&ulib::uverbs::CompletionQueue, super::ControlPathError> {
        if self.local_resource().cq.is_none() {
            // create a CQ on the same NIC
            let cmid_verbs_ctx = builder.get_default_verbs_context()?;
            let cq = cmid_verbs_ctx.create_cq(cq_size, cq_context).unwrap();
            self.local_resource.cq = Some(cq);
        }

        // Check whether the existing CQ is on the same NIC as the new cmid
        let cq = self.local_resource().cq.as_ref().unwrap();
        let cmid_verbs_ctx = builder.get_default_verbs_context()?;
        assert_eq!(
            cmid_verbs_ctx.as_handle(),
            cq.get_verbs_context().unwrap().as_handle()
        );

        Ok(cq)
    }
}
