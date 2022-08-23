//! Runtime manager is the control plane of runtimes. It is responsible for
//! creating/destructing runtimes, map runtimes to cores, balance the work
//! among different runtimes, and even dynamically scale out/down the runtimes.
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::{self, JoinHandle};

use dashmap::DashMap;
use interface::engine::SchedulingMode;
use nix::unistd::Pid;

use super::container::EngineContainer;
use super::datapath::graph::DataPathGraph;
use super::runtime::{self, Runtime};
use super::EngineType;
use crate::config::Config;
use crate::module::Service;
use crate::storage::ResourceCollection;

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EngineId(pub(crate) u64);

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RuntimeId(pub(crate) u64);

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GroupId(pub(crate) u64);

#[derive(Debug, Clone, Copy)]
pub(crate) struct EngineInfo {
    /// Application process PID that this engine serves
    pub(crate) pid: Pid,
    /// Which group the engine belongs to,
    pub(crate) gid: GroupId,
    /// Runtime ID the engine is running on
    pub(crate) rid: RuntimeId,
    /// The type of the engine
    pub(crate) engine_type: EngineType,
    /// Scheduling mode
    pub(crate) scheduling_mode: SchedulingMode,
}

pub(crate) struct ServiceSubscription {
    pub(crate) service: Service,
    pub(crate) addons: Vec<EngineType>,
    pub(crate) graph: DataPathGraph,
}

pub(crate) struct GlobalResourceManager {
    pub(crate) resource: DashMap<Pid, ResourceCollection>,
    /// Number of active engine groups for each application process
    active_cnt: DashMap<Pid, usize>,
}

impl GlobalResourceManager {
    fn new() -> Self {
        GlobalResourceManager {
            resource: DashMap::new(),
            active_cnt: DashMap::new(),
        }
    }

    #[inline]
    pub(crate) fn register_group_shutdown(&self, pid: Pid) {
        let removed = self.active_cnt.remove_if_mut(&pid, |_, cnt| {
            *cnt -= 1;
            *cnt == 0
        });
        if removed.is_some() {
            self.resource.remove(&pid);
        }
    }
}

pub struct RuntimeManager {
    pub inner: Mutex<Inner>,
    /// Per-process counter for generating ID of engine groups
    group_counter: DashMap<Pid, u64>,
    /// Info about the engines
    pub(crate) engine_subscriptions: DashMap<EngineId, EngineInfo>,
    /// The service each engine group is running,
    /// and the number of active engines in that group
    pub(crate) service_subscriptions: DashMap<(Pid, GroupId), (ServiceSubscription, usize)>,
    pub(crate) global_resource_mgr: GlobalResourceManager,
}

pub struct Inner {
    // the number of engines are at most u64::MAX
    engine_counter: u64,
    // the number of runtimes are at most u64::MAX
    runtime_counter: u64,
    pub(crate) runtimes: HashMap<RuntimeId, Arc<Runtime>>,
    handles: HashMap<RuntimeId, JoinHandle<Result<(), runtime::Error>>>,
}

impl Inner {
    fn schedule_dedicate(
        &mut self,
        engine: EngineContainer,
        rm: &Arc<RuntimeManager>,
    ) -> (EngineId, RuntimeId) {
        // find a spare runtime
        // NOTE(wyj): iterating over HashMap should be fine
        // as submitting a new engine is not on fast path
        // Moreover, there are only limited number of runtimes
        let rid = match self.runtimes.iter().find(|(_i, r)| r.is_empty()) {
            Some((rid, _runtime)) => *rid,
            None => self.start_runtime(self.runtime_counter as usize, Arc::clone(rm)),
        };

        let engine_id = EngineId(self.engine_counter);
        self.engine_counter = self.engine_counter.checked_add(1).unwrap();
        self.runtimes[&rid].add_engine(engine_id, engine);

        // a runtime will not be parked when having pending engines, so in theory, we can check
        // whether the runtime and only unpark it when it's in parked state.
        self.handles[&rid].thread().unpark();

        (engine_id, rid)
    }
}

impl RuntimeManager {
    pub fn new(_config: &Config) -> Self {
        let inner = Inner {
            engine_counter: 0,
            runtime_counter: 0,
            runtimes: HashMap::with_capacity(1),
            handles: HashMap::with_capacity(1),
        };
        RuntimeManager {
            inner: Mutex::new(inner),
            group_counter: DashMap::new(),
            engine_subscriptions: DashMap::new(),
            service_subscriptions: DashMap::new(),
            global_resource_mgr: GlobalResourceManager::new(),
        }
    }

    pub(crate) fn submit(
        self: &Arc<Self>,
        pid: Pid,
        gid: GroupId,
        engine: EngineContainer,
        mode: SchedulingMode,
        register_subscription: bool,
    ) {
        let mut inner = self.inner.lock().unwrap();
        match mode {
            SchedulingMode::Dedicate => {
                let engine_type = engine.engine_type();
                let (eid, rid) = inner.schedule_dedicate(engine, self);
                let engine_info = EngineInfo {
                    pid,
                    gid,
                    rid,
                    scheduling_mode: mode,
                    engine_type,
                };
                let prev = self.engine_subscriptions.insert(eid, engine_info);
                assert!(prev.is_none(), "eid={:?} is already used", eid);
                if register_subscription {
                    // increase active engine count for corresponding engine group
                    self.service_subscriptions.get_mut(&(pid, gid)).unwrap().1 += 1;
                }
            }
            SchedulingMode::Compact => unimplemented!(),
            SchedulingMode::Spread => unimplemented!(),
        }
    }

    /// Create a new engine group for service subscription
    pub(crate) fn new_group(&self, pid: Pid, subscription: ServiceSubscription) -> GroupId {
        let mut counter = self.group_counter.entry(pid).or_insert(0);
        let gid = GroupId(*counter);
        self.service_subscriptions
            .insert((pid, gid), (subscription, 0));
        *self.global_resource_mgr.active_cnt.entry(pid).or_insert(0) += 1;
        *counter = counter.checked_add(1).unwrap();
        gid
    }

    pub(crate) fn register_engine_shutdown(&self, engine_id: EngineId) {
        let info = self.engine_subscriptions.remove(&engine_id).unwrap().1;
        let removed =
            self.service_subscriptions
                .remove_if_mut(&(info.pid, info.gid), |_, (_, cnt)| {
                    *cnt -= 1;
                    *cnt == 0
                });
        if removed.is_some() {
            self.global_resource_mgr.register_group_shutdown(info.pid);
        }
    }
}

impl Inner {
    fn start_runtime(&mut self, _core: usize, rm: Arc<RuntimeManager>) -> RuntimeId {
        let runtime_id = RuntimeId(self.runtime_counter);
        self.runtime_counter = self.runtime_counter.checked_add(1).unwrap();
        let runtime = Arc::new(Runtime::new(runtime_id, rm));
        self.runtimes.insert(runtime_id, Arc::clone(&runtime));

        let handle = thread::Builder::new()
            .name(format!("Runtime {}", runtime_id.0))
            .spawn(move || {
                // check core id
                // let num_cpus = num_cpus::get();
                // if core >= num_cpus {
                //     return Err(runtime::Error::InvalidId(core));
                // }
                // NOTE(cjr): do not set affinity here. It only hurts the performance if the user app
                // does not run on the hyperthread core pair. Since we cannot expect that we always
                // have hyperthread core pair available, not setting affinity turns out to be better.
                // scheduler::set_self_affinity(scheduler::CpuSet::single(core))
                //     .map_err(|_| runtime::Error::SetAffinity(io::Error::last_os_error()))?;
                runtime.mainloop()
            })
            .unwrap_or_else(|e| panic!("failed to spawn new threads: {}", e));

        self.handles.insert(runtime_id, handle);
        runtime_id
    }
}
