use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use futures::executor::{ThreadPool, ThreadPoolBuilder};
use interface::engine::SchedulingMode;
use nix::unistd::Pid;

use super::manager::{EngineId, RuntimeId, RuntimeManager};
use super::runtime::SuspendResult;
use super::{EngineContainer, EngineType};
use crate::plugin::PluginCollection;

pub struct EngineUpgrader {
    runtime_manager: Arc<RuntimeManager>,
    plugins: Arc<PluginCollection>,
    executor: ThreadPool,
}

async fn upgrade_client(
    rm: Arc<RuntimeManager>,
    plugins: Arc<PluginCollection>,
    pid: Pid,
    engines: Vec<(EngineId, EngineType, RuntimeId)>,
) {
    let mut pending = Vec::with_capacity(engines.len());

    for (engine_id, engine_type, runtime_id) in engines {
        let guard = rm.inner.lock().unwrap();
        let runtime = guard.runtimes.get(&runtime_id).unwrap();
        runtime.request_suspend(engine_id);
        pending.push((engine_id, engine_type, runtime_id));
    }

    // TODO(wyj): topo order
    // TODO(wyj): shared states, global states
    let mut local_states = HashMap::with_capacity(pending.len());
    while pending.len() > 0 {
        pending.retain(|(eid, engine_type, rid)| {
            let guard = rm.inner.lock().unwrap();
            let runtime = guard.runtimes.get(rid).unwrap();
            if let Some((_, result)) = runtime.suspended.remove(eid) {
                if let SuspendResult::Engine(container) = result {
                    let engine = container.detach();
                    eprintln!("Dumping engine states...");
                    let state = engine.unload();
                    local_states.insert(*eid, (engine_type.clone(), state));
                }
                false
            } else {
                true
            }
        })
    }

    for (_eid, (engine_type, state)) in local_states {
        let module_name = plugins.engine_registry.get(&engine_type).unwrap();

        let module = plugins.modules.get_mut(module_name.value()).unwrap();
        let engine = module.restore_engine(&engine_type, state).unwrap();
        let container = EngineContainer::new_v2(engine);
        eprintln!("Restore engine and submit to runtime...");
        rm.submit(pid, container, SchedulingMode::Dedicate);
    }
}

impl EngineUpgrader {
    pub fn new(rm: Arc<RuntimeManager>, plugins: Arc<PluginCollection>) -> Self {
        let pool = ThreadPoolBuilder::new().pool_size(1).create().unwrap();
        EngineUpgrader {
            runtime_manager: rm,
            plugins: plugins,
            executor: pool,
        }
    }

    pub fn upgrade(&mut self, to_upgrade: HashSet<EngineType>) {
        let mut clients_pending_upgrade = Vec::new();

        for client in self.runtime_manager.clients.iter() {
            let pid = *client.key();
            let engines = client.value();
            let filtered = engines
                .iter()
                .filter_map(|x| {
                    let (engine_type, runtime_id) = x.value();
                    if to_upgrade.contains(engine_type) {
                        Some((*x.key(), engine_type.clone(), *runtime_id))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            clients_pending_upgrade.push((pid, filtered));
        }

        for (pid, engines) in clients_pending_upgrade {
            let rm = Arc::clone(&self.runtime_manager);
            let plugins = Arc::clone(&self.plugins);
            let fut = upgrade_client(rm, plugins, pid, engines);
            self.executor.spawn_ok(fut);
        }
    }
}
