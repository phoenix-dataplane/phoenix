use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::bail;
use dashmap::DashSet;
use futures::executor::{ThreadPool, ThreadPoolBuilder};
use nix::unistd::Pid;

use super::manager::{EngineId, EngineInfo, RuntimeManager};
use super::runtime::SuspendResult;
use super::{EngineContainer, EngineType};
use crate::plugin::PluginCollection;
use crate::storage::{ResourceCollection, SharedStorage};

pub struct EngineUpgrader {
    runtime_manager: Arc<RuntimeManager>,
    plugins: Arc<PluginCollection>,
    executor: ThreadPool,
    upgrade_indicator: Arc<DashSet<Pid>>,
}

async fn upgrade_client(
    rm: Arc<RuntimeManager>,
    plugins: Arc<PluginCollection>,
    pid: Pid,
    mut pending: Vec<(EngineId, EngineInfo)>,
    indicator: Arc<DashSet<Pid>>,
) {
    for (engine_id, info) in pending.iter() {
        let guard = rm.inner.lock().unwrap();
        let runtime = guard.runtimes.get(&info.rid).unwrap();
        runtime.request_suspend(*engine_id);
    }

    // Each engine's local state
    let mut local_states = HashMap::new();
    // Shared storage for each engine group
    let mut shared_storage = HashMap::new();
    // Global resources that are shared by all engines
    let mut global_resource = rm
        .global_resource_mgr
        .resource
        .entry(pid)
        .or_insert_with(ResourceCollection::new);
    while pending.len() > 0 {
        pending.retain(|(eid, info)| {
            let guard = rm.inner.lock().unwrap();
            let runtime = guard.runtimes.get(&info.rid).unwrap();
            if let Some((_, result)) = runtime.suspended.remove(eid) {
                if let SuspendResult::Engine(container) = result {
                    let shared = shared_storage
                        .entry(info.gid)
                        .or_insert_with(SharedStorage::new);
                    let prev_version = container.version();
                    let engine = container.detach();
                    let state = engine.unload(shared, global_resource.value_mut());
                    rm.register_engine_shutdown(*eid);
                    tracing::trace!(
                        "Engine (pid={:?}, eid={:?}, gid={:?}) detached",
                        pid,
                        eid,
                        info.gid
                    );
                    let entry = local_states.entry(info.gid).or_insert_with(HashMap::new);
                    let prev = entry.insert(
                        info.engine_type.clone(),
                        (*eid, state, prev_version, info.scheduling_mode),
                    );
                    // Within each group, there is at most one engine for each type
                    debug_assert!(prev.is_none());
                }
                false
            } else {
                true
            }
        })
    }

    for (gid, mut engine_group) in local_states {
        // Koala control and runtime manager's implementation ensures that we can always find the corresponding service.
        let (service, _) = rm.service_subscriptions.remove(&(pid, gid)).unwrap().1;
        let mut shared = shared_storage.remove(&gid).unwrap();
        let service_engines = plugins.service_registry.get(&service).unwrap();

        let new_gid = rm.get_new_group_id(pid, service);
        for engine_type in service_engines.value().iter() {
            if let Some((eid, state, prev_version, mode)) = engine_group.remove(&engine_type) {
                let module_name = plugins.engine_registry.get(&engine_type).unwrap();
                let mut module = plugins.modules.get_mut(module_name.value()).unwrap();
                let version = module.version();
                let engine = module.restore_engine(
                    &engine_type,
                    state,
                    &mut shared,
                    global_resource.value_mut(),
                    &plugins.modules,
                    prev_version,
                );
                match engine {
                    Ok(engine) => {
                        let container =
                            EngineContainer::new_v2(engine, engine_type.clone(), version);
                        rm.submit(pid, new_gid, container, mode);
                        tracing::trace!("Engine (pid={:?}, prev_eid={:?}, prev_gid={:?}, type: {:?}) restored and submit to runtime manager", pid, eid, gid, engine_type);
                        eprintln!("Engine (pid={:?}, prev_eid={:?}, prev_gid={:?}, type: {:?}) restored and submit to runtime manager", pid, eid, gid, engine_type);
                    }
                    Err(_) => {
                        log::warn!(
                            "Failed to restore engine (pid={:?}, eid={:?}, gid={:?}) of type {:?}",
                            pid,
                            eid,
                            gid,
                            engine_type
                        );
                    }
                }
            }
        }
    }

    indicator.remove(&pid);
    if indicator.is_empty() {
        plugins.upgrade_cleanup();
    }
}

impl EngineUpgrader {
    pub fn new(rm: Arc<RuntimeManager>, plugins: Arc<PluginCollection>) -> Self {
        let pool = ThreadPoolBuilder::new().pool_size(1).create().unwrap();
        EngineUpgrader {
            runtime_manager: rm,
            plugins: plugins,
            executor: pool,
            upgrade_indicator: Arc::new(DashSet::new()),
        }
    }

    pub fn upgrade(&mut self, engine_types: HashSet<EngineType>) -> anyhow::Result<()> {
        if !self.upgrade_indicator.is_empty() {
            bail!("there is already an ongoing upgrade")
        }
        let mut clients_to_upgrade = HashMap::new();

        for engine in self
            .runtime_manager
            .engine_subscriptions
            .iter()
            .filter(|e| engine_types.contains(&e.engine_type))
        {
            let client = clients_to_upgrade
                .entry(engine.pid)
                .or_insert_with(Vec::new);
            client.push((*engine.key(), engine.value().clone()));
        }

        for (pid, engines) in clients_to_upgrade {
            let rm = Arc::clone(&self.runtime_manager);
            let plugins = Arc::clone(&self.plugins);
            let fut = upgrade_client(rm, plugins, pid, engines, self.upgrade_indicator.clone());
            self.executor.spawn_ok(fut);
        }

        Ok(())
    }

    /// Check whether engines for an application process is still upgrading,
    /// returns true if still upgrading
    pub(crate) fn is_upgrading(&self, pid: Pid) -> bool {
        self.upgrade_indicator.contains(&pid)
    }
}
