use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::bail;
use dashmap::DashSet;
use futures::executor::{ThreadPool, ThreadPoolBuilder};
use nix::unistd::Pid;
use semver::Version;

use interface::engine::SchedulingMode;

use super::datapath::graph::ChannelDescriptor;
use super::datapath::node::{refactor_channels_add_plugin, DataPathNode};
use super::manager::{EngineId, EngineInfo, GroupId, RuntimeManager};
use super::runtime::SuspendResult;
use super::{EngineContainer, EngineType};
use crate::plugin::{Plugin, PluginCollection};
use crate::storage::{ResourceCollection, SharedStorage};

pub struct EngineUpgrader {
    runtime_manager: Arc<RuntimeManager>,
    plugins: Arc<PluginCollection>,
    executor: ThreadPool,
    upgrade_indicator: Arc<DashSet<Pid>>,
}

struct EngineDumped {
    local_states: ResourceCollection,
    node: DataPathNode,
    prev_version: Version,
    mode: SchedulingMode,
}

async fn add_plugin(
    rm: Arc<RuntimeManager>,
    plugins: Arc<PluginCollection>,
    pid: Pid,
    gid: GroupId,
    // addon: EngineType,
) {
    let mut group_engines = rm
        .engine_subscriptions
        .iter()
        .filter(|e| e.pid == pid && e.gid == gid)
        .map(|e| (*e.key(), e.value().clone()))
        .collect::<Vec<_>>();

    {
        let guard = rm.inner.lock().unwrap();
        for (engine_id, info) in group_engines.iter() {
            let runtime = guard.runtimes.get(&info.rid).unwrap();
            runtime.request_suspend(*engine_id);
        }
    }

    let (mut subscription, _) = rm.service_subscriptions.remove(&(pid, gid)).unwrap().1;
    let mut engine_containers = Vec::with_capacity(group_engines.len());
    while group_engines.len() > 0 {
        let guard = rm.inner.lock().unwrap();
        group_engines.retain(|(eid, info)| {
            let runtime = guard.runtimes.get(&info.rid).unwrap();
            if let Some((_, result)) = runtime.suspended.remove(&eid) {
                if let SuspendResult::Engine(container) = result {
                    engine_containers.push((container, info.scheduling_mode));
                    rm.register_engine_shutdown(*eid);
                }
                false
            } else {
                true
            }
        });
    }

    // let containers_resubmit = Vec::with_capacity(engine_containers.len() + 1);
    let mut detached_engines = HashMap::with_capacity(engine_containers.len());
    for (container, mode) in engine_containers {
        let engine_type = container.engine_type().clone();
        let mut engine = container.detach();
        engine.detach();
        detached_engines.insert(engine_type, engine);
    }

    let mut tx_edges = Vec::new();
    tx_edges.push(ChannelDescriptor(
        EngineType(String::from("MrpcEngine")),
        EngineType(String::from("RateLimitEngine")),
        0,
        0,
    ));
    tx_edges.push(ChannelDescriptor(
        EngineType(String::from("RateLimitEngine")),
        EngineType(String::from("RpcAdapterEngine")),
        0,
        0,
    ));

    let rx_edges = Vec::new();
    let node = refactor_channels_add_plugin(
        &mut detached_engines,
        &mut subscription.graph,
        EngineType(String::from("RateLimitEngine")),
        tx_edges,
        rx_edges,
    )
    .unwrap();

    eprintln!("new data node created");
    let mut containers_resubmit = Vec::new();
    let mut rate_limiter = plugins.addons.get_mut("RateLimit").unwrap();
    let engine_type = EngineType(String::from("RateLimitEngine"));
    eprintln!("prepare to create rate limiter");
    let addon_engine = rate_limiter
        .value_mut()
        .create_engine(&engine_type, pid, node)
        .unwrap();
    eprintln!("new addon engine created");

    let container =
        EngineContainer::new(addon_engine, engine_type, Version::parse("0.1.0").unwrap());
    containers_resubmit.push(container);

    for (ty, engine) in detached_engines {
        let container = EngineContainer::new(engine, ty, Version::parse("0.1.0").unwrap());
        containers_resubmit.push(container);
    }

    let new_gid = rm.new_group(pid, subscription);
    for container in containers_resubmit {
        eprintln!(
            "submit container to runtime, engine ty={:?}",
            container.engine_type()
        );
        rm.submit(pid, new_gid, container, SchedulingMode::Dedicate);
    }
    rm.global_resource_mgr.register_group_shutdown(pid);
}

async fn upgrade_client(
    rm: Arc<RuntimeManager>,
    plugins: Arc<PluginCollection>,
    pid: Pid,
    mut upgrade: Vec<(EngineId, EngineInfo)>,
    mut others: Vec<(EngineId, EngineInfo)>,
    indicator: Arc<DashSet<Pid>>,
) {
    {
        let guard = rm.inner.lock().unwrap();
        for (engine_id, info) in upgrade.iter().chain(others.iter()) {
            let runtime = guard.runtimes.get(&info.rid).unwrap();
            runtime.request_suspend(*engine_id);
        }
    }

    // EngineContainers detached from runtimes, awaiting for upgrade
    let mut upgrade_containers = HashMap::new();
    // EngineContainers for other engines within the same engine groups
    // that do not need update
    let mut others_containers = HashMap::new();
    let mut service_subscriptions = HashMap::new();
    while upgrade.len() > 0 || others.len() > 0 {
        let guard = rm.inner.lock().unwrap();
        upgrade.retain(|(eid, info)| {
            let runtime = guard.runtimes.get(&info.rid).unwrap();
            if let Some((_, result)) = runtime.suspended.remove(eid) {
                if let SuspendResult::Engine(container) = result {
                    let group = upgrade_containers.entry(info.gid).or_insert_with(Vec::new);
                    group.push((container, info.scheduling_mode));
                    if let Entry::Vacant(entry) = service_subscriptions.entry(info.gid) {
                        // if an engine is successfully suspended from runtime
                        // then the corresponding entry about its group
                        // must still be valid in `rm.service_subscriptions`
                        let (subscription, _) =
                            rm.service_subscriptions.remove(&(pid, info.gid)).unwrap().1;
                        entry.insert(subscription);
                    }
                    rm.register_engine_shutdown(*eid);
                }
                // if all engines with in the group is already shutdown
                // the entry from `rm.service_subscriptions` should have already been removed
                // if the group is the last active group for pid,
                // the entry in `rm.global_resource_mgr` has also been removed.
                false
            } else {
                true
            }
        });
        others.retain(|(eid, info)| {
            let runtime = guard.runtimes.get(&info.rid).unwrap();
            if let Some((_, result)) = runtime.suspended.remove(eid) {
                if let SuspendResult::Engine(container) = result {
                    let group = others_containers.entry(info.gid).or_insert_with(Vec::new);
                    group.push((container, info.scheduling_mode));
                    if let Entry::Vacant(entry) = service_subscriptions.entry(info.gid) {
                        let (subscription, _) =
                            rm.service_subscriptions.remove(&(pid, info.gid)).unwrap().1;
                        entry.insert(subscription);
                    }
                    rm.register_engine_shutdown(*eid);
                }
                false
            } else {
                true
            }
        });
    }

    // Decompose the engines to upgrade
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

    for (gid, containers) in upgrade_containers {
        let shared = shared_storage.entry(gid).or_insert_with(SharedStorage::new);
        for (container, mode) in containers {
            let prev_version = container.version();
            let engine_type = container.engine_type().clone();
            let mut engine = container.detach();
            engine.detach();
            let (state, node) = engine.unload(shared, global_resource.value_mut());
            tracing::trace!(
                "Engine (pid={:?}, gid={:?}, type={:?}) dumped",
                pid,
                gid,
                engine_type,
            );
            let entry = local_states.entry(gid).or_insert_with(HashMap::new);
            let dumped = EngineDumped {
                local_states: state,
                node,
                prev_version,
                mode,
            };
            entry.insert(engine_type, dumped).unwrap();
        }
    }

    for (gid, subscription) in service_subscriptions {
        // TODO: later, when we change EngineType to &'static str,
        // we also need to make sure to change subscription
        // to ensure there is not static str pointed to the old library

        // Koala control and runtime manager's implementation ensures that we can always find the corresponding service.
        let service = plugins.service_registry.get(&subscription.service).unwrap();

        let mut resubmit = true;
        let mut containers_to_submit = if let Some(containers) = others_containers.remove(&gid) {
            containers
        } else {
            Vec::with_capacity(service.engines.len())
        };
        if let Some(mut engine_group) = local_states.remove(&gid) {
            let mut shared = shared_storage.remove(&gid).unwrap();
            for engine_type in service.engines.iter().chain(subscription.addons.iter()) {
                if let Some(dumped) = engine_group.remove(&engine_type) {
                    let plugin = plugins.engine_registry.get(&engine_type).unwrap();
                    let (engine, version) = match plugin.value() {
                        Plugin::Module(module_name) => {
                            let mut module = plugins.modules.get_mut(module_name).unwrap();
                            let version = module.version();
                            // resotre engine
                            let engine = module.restore_engine(
                                &engine_type,
                                dumped.local_states,
                                &mut shared,
                                global_resource.value_mut(),
                                dumped.node,
                                &plugins.modules,
                                dumped.prev_version,
                            );
                            (engine, version)
                        }
                        Plugin::Addon(addon_name) => {
                            let mut addon = plugins.addons.get_mut(addon_name).unwrap();
                            let version = addon.version();
                            let engine = addon.restore_engine(
                                &engine_type,
                                dumped.local_states,
                                dumped.node,
                                dumped.prev_version,
                            );
                            (engine, version)
                        }
                    };
                    match engine {
                        Ok(engine) => {
                            let container =
                                EngineContainer::new(engine, engine_type.clone(), version);
                            containers_to_submit.push((container, dumped.mode));
                            tracing::trace!(
                                "Engine (pid={:?}, (prev) gid={:?}, type={:?}) restored",
                                pid,
                                gid,
                                engine_type
                            );
                            // TODO: remove
                            eprintln!(
                                "Engine (pid={:?}, (prev) gid={:?}, type={:?}) restored",
                                pid, gid, engine_type
                            );
                        }
                        Err(err) => {
                            tracing::error!(
                                "Failed to restore engine (pid={:?}, (prev) gid={:?}, type={:?})",
                                pid,
                                gid,
                                engine_type
                            );
                            resubmit = false;
                            break;
                        }
                    }
                }
            }
        }

        if resubmit {
            // create a new group
            let new_gid = rm.new_group(pid, subscription);
            for (container, mode) in containers_to_submit {
                rm.submit(pid, new_gid, container, mode);
            }
        }
        // register the old group has shutdown, remove it from global resource's `active_cnt`
        rm.global_resource_mgr.register_group_shutdown(pid);
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

    pub fn add_addon(&mut self) {
        assert_eq!(self.runtime_manager.service_subscriptions.len(), 2);
        let mut pid = None;
        let mut gid = None;
        for client in self.runtime_manager.service_subscriptions.iter() {
            if &client.value().0.service.0 == "Mrpc" {
                pid = Some(client.key().0);
                gid = Some(client.key().1)
            }
        }
        let pid = pid.unwrap();
        let gid = gid.unwrap();

        let fut = add_plugin(self.runtime_manager.clone(), self.plugins.clone(), pid, gid);
        self.executor.spawn_ok(fut);
    }

    pub fn upgrade(&mut self, engine_types: HashSet<EngineType>) -> anyhow::Result<()> {
        if !self.upgrade_indicator.is_empty() {
            bail!("there is already an ongoing upgrade")
        }
        // engines that need to be upgraded
        let mut engines_to_upgrade = HashMap::new();
        // other engines that are in the same group
        // as the engines to be upgraded
        let mut engines_to_suspend = HashMap::new();

        let mut groups_to_upgrade = HashSet::new();
        for engine in self
            .runtime_manager
            .engine_subscriptions
            .iter()
            .filter(|e| engine_types.contains(&e.engine_type))
        {
            let client = engines_to_upgrade
                .entry(engine.pid)
                .or_insert_with(Vec::new);
            client.push((*engine.key(), engine.value().clone()));
            groups_to_upgrade.insert((engine.pid, engine.gid));
        }

        for engine in self
            .runtime_manager
            .engine_subscriptions
            .iter()
            .filter(|e| {
                !engine_types.contains(&e.engine_type)
                    && groups_to_upgrade.contains(&(e.pid, e.gid))
            })
        {
            let client = engines_to_suspend
                .entry(engine.pid)
                .or_insert_with(Vec::new);
            client.push((*engine.key(), engine.value().clone()));
        }

        for (pid, to_upgrade) in engines_to_upgrade {
            let to_suspend = if let Some(engines) = engines_to_suspend.remove(&pid) {
                engines
            } else {
                Vec::new()
            };
            let rm = Arc::clone(&self.runtime_manager);
            let plugins = Arc::clone(&self.plugins);
            let fut = upgrade_client(
                rm,
                plugins,
                pid,
                to_upgrade,
                to_suspend,
                self.upgrade_indicator.clone(),
            );
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
