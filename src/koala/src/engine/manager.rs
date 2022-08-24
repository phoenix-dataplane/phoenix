//! Runtime manager is the control plane of runtimes. It is responsible for
//! creating/destructing runtimes, map runtimes to cores, balance the work
//! among different runtimes, and even dynamically scale out/down the runtimes.
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::{self, JoinHandle};

use dashmap::DashMap;
use interface::engine::SchedulingMode;
use nix::unistd::Pid;

use super::container::EngineContainer;
use super::datapath::graph::DataPathGraph;
use super::group::GroupId;
use super::runtime::{self, Runtime};
use super::EngineType;
use super::SchedulingGroup;
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
pub(crate) struct SubscriptionId(pub(crate) u64);

#[derive(Debug, Clone, Copy)]
pub(crate) struct EngineInfo {
    /// Application process PID that this engine serves
    pub(crate) pid: Pid,
    /// Which service subscription the engine belongs to,
    pub(crate) sid: SubscriptionId,
    /// Runtime ID the engine is running on
    pub(crate) rid: RuntimeId,
    /// Which scheduling group the engine belongs to,
    pub(crate) gid: GroupId,
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
    pub(crate) fn register_subscription_shutdown(&self, pid: Pid) {
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
    // EngineId counter
    // the number of engines are at most u64::MAX
    pub(crate) engine_counter: AtomicU64,
    // GroupId counter
    pub(crate) scheduling_group_counter: AtomicU64,
    /// Per-process counter for generating ID of engine groups
    subscription_counter: DashMap<Pid, u64>,
    // TODO: decide whether to move some of the other fields into Inner.
    pub inner: Mutex<Inner>,
    /// Info about the engines
    pub(crate) engine_subscriptions: DashMap<EngineId, EngineInfo>,
    /// The service each engine group is running,
    /// and the number of active engines in that group
    pub(crate) service_subscriptions: DashMap<(Pid, SubscriptionId), (ServiceSubscription, usize)>,
    pub(crate) global_resource_mgr: GlobalResourceManager,
}

pub struct Inner {
    // the number of runtimes are at most u64::MAX
    runtime_counter: u64,
    pub(crate) runtimes: HashMap<RuntimeId, Arc<Runtime>>,
    handles: HashMap<RuntimeId, JoinHandle<Result<(), runtime::Error>>>,
}

impl Inner {
    fn schedule_dedicate(
        &mut self,
        pid: Pid,
        sid: SubscriptionId,
        group: SchedulingGroup,
        rm: &Arc<RuntimeManager>,
    ) {
        // find a spare runtime
        // NOTE(wyj): iterating over HashMap should be fine
        // as submitting a new engine is not on fast path
        // Moreover, there are only limited number of runtimes
        let rid = match self.runtimes.iter().find(|(_i, r)| r.is_empty()) {
            Some((rid, _runtime)) => *rid,
            None => self.start_runtime(self.runtime_counter as usize, Arc::clone(rm)),
        };

        for (eid, engine) in group.engines.iter() {
            let engine_type = engine.engine_type();
            let engine_info = EngineInfo {
                pid,
                sid,
                rid,
                gid: group.id,
                scheduling_mode: SchedulingMode::Dedicate,
                engine_type,
            };
            tracing::info!(
                "Submitting engine {:?} (pid={:?}, sid={:?}, gid={:?}) to runtime (rid={:?})",
                engine_type,
                pid,
                sid,
                group.id,
                rid,
            );
            let prev = rm.engine_subscriptions.insert(*eid, engine_info);
            assert!(prev.is_none(), "eid={:?} is already used", eid);
        }

        self.runtimes[&rid].add_group(group, true);
        // a runtime will not be parked when having pending engines, so in theory, we can check
        // whether the runtime and only unpark it when it's in parked state.
        self.handles[&rid].thread().unpark();
    }

    fn schedule_compact(
        &mut self,
        pid: Pid,
        sid: SubscriptionId,
        group: SchedulingGroup,
        rm: &Arc<RuntimeManager>,
    ) {
        // find a spare runtime
        // NOTE(wyj): iterating over HashMap should be fine
        // as submitting a new engine is not on fast path
        // Moreover, there are only limited number of runtimes
        let rid = match self.runtimes.iter().find(|(_i, r)| r.is_empty() || !r.is_dedicated()) {
            Some((rid, _runtime)) => *rid,
            None => self.start_runtime(self.runtime_counter as usize, Arc::clone(rm)),
        };

        for (eid, engine) in group.engines.iter() {
            let engine_type = engine.engine_type();
            let engine_info = EngineInfo {
                pid,
                sid,
                rid,
                gid: group.id,
                scheduling_mode: SchedulingMode::Dedicate,
                engine_type,
            };
            tracing::info!(
                "Submitting engine {:?} (pid={:?}, sid={:?}, gid={:?}) to runtime (rid={:?})",
                engine_type,
                pid,
                sid,
                group.id,
                rid,
            );
            let prev = rm.engine_subscriptions.insert(*eid, engine_info);
            assert!(prev.is_none(), "eid={:?} is already used", eid);
        }

        self.runtimes[&rid].add_group(group, false);
        // a runtime will not be parked when having pending engines, so in theory, we can check
        // whether the runtime and only unpark it when it's in parked state.
        self.handles[&rid].thread().unpark();
    }
}

impl RuntimeManager {
    pub fn new(_config: &Config) -> Self {
        let inner = Inner {
            runtime_counter: 0,
            runtimes: HashMap::with_capacity(1),
            handles: HashMap::with_capacity(1),
        };
        RuntimeManager {
            engine_counter: AtomicU64::new(0),
            scheduling_group_counter: AtomicU64::new(0),
            inner: Mutex::new(inner),
            subscription_counter: DashMap::new(),
            engine_subscriptions: DashMap::new(),
            service_subscriptions: DashMap::new(),
            global_resource_mgr: GlobalResourceManager::new(),
        }
    }

    pub(crate) fn attach_to_group(
        self: &Arc<Self>,
        pid: Pid,
        sid: SubscriptionId,
        gid: GroupId,
        rid: RuntimeId,
        engines: Vec<EngineContainer>,
        mode: SchedulingMode,
    ) {
        let inner = self.inner.lock().unwrap();
        let mut submission = Vec::with_capacity(engines.len());
        for engine in engines {
            let eid = EngineId(self.engine_counter.fetch_add(1, Ordering::Relaxed));
            let engine_type = engine.engine_type();
            let engine_info = EngineInfo {
                pid,
                sid,
                rid,
                gid,
                scheduling_mode: mode,
                engine_type,
            };
            tracing::info!(
                "Attaching engine {:?} (pid={:?}, sid={:?}, gid={:?}) to runtime (rid={:?})",
                engine_type,
                pid,
                sid,
                gid,
                rid,
            );
            let prev = self.engine_subscriptions.insert(eid, engine_info);
            assert!(prev.is_none(), "eid={:?} is already used", eid);
            submission.push((eid, engine));
        }
        inner.runtimes[&rid].attach_engines_to_group(gid, submission);
    }

    pub(crate) fn submit_group(
        self: &Arc<Self>,
        pid: Pid,
        sid: SubscriptionId,
        engines: Vec<EngineContainer>,
        mode: SchedulingMode,
    ) {
        let mut inner = self.inner.lock().unwrap();
        let mut submission = Vec::with_capacity(engines.len());
        for engine in engines {
            let eid = EngineId(self.engine_counter.fetch_add(1, Ordering::Relaxed));
            submission.push((eid, engine));
        }
        let gid = GroupId(
            self.scheduling_group_counter
                .fetch_add(1, Ordering::Relaxed),
        );
        let group = SchedulingGroup::new(gid, submission);

        match mode {
            SchedulingMode::Dedicate => {
                inner.schedule_dedicate(pid, sid, group, self);
            }
            SchedulingMode::Compact => {
                inner.schedule_compact(pid, sid, group, self);
            }
            SchedulingMode::Spread => unimplemented!(),
        }
    }

    /// Create a new engine group for service subscription
    pub(crate) fn new_subscription(
        &self,
        pid: Pid,
        subscription: ServiceSubscription,
    ) -> SubscriptionId {
        let mut counter = self.subscription_counter.entry(pid).or_insert(0);
        let sid = SubscriptionId(*counter);
        self.service_subscriptions
            .insert((pid, sid), (subscription, 0));
        *self.global_resource_mgr.active_cnt.entry(pid).or_insert(0) += 1;
        *counter = counter.checked_add(1).unwrap();
        sid
    }

    pub(crate) fn register_engine_shutdown(&self, engine_id: EngineId) {
        let info = self.engine_subscriptions.remove(&engine_id).unwrap().1;
        let removed =
            self.service_subscriptions
                .remove_if_mut(&(info.pid, info.sid), |_, (_, cnt)| {
                    *cnt -= 1;
                    *cnt == 0
                });
        if removed.is_some() {
            self.global_resource_mgr
                .register_subscription_shutdown(info.pid);
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
