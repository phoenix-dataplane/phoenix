use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::bail;
use dashmap::DashSet;
use futures::executor::{ThreadPool, ThreadPoolBuilder};
use nix::unistd::Pid;
use semver::Version;

use interface::engine::SchedulingMode;

use super::datapath::{DataPathNode, ChannelDescriptor};
use super::datapath::{refactor_channels_attach_addon, refactor_channels_detach_addon};
use super::manager::{EngineId, EngineInfo, GroupId, RuntimeManager};
use super::runtime::SuspendResult;
use super::{EngineContainer, EngineType};
use crate::plugin::{Plugin, PluginCollection};
use crate::storage::{ResourceCollection, SharedStorage};

pub(crate) struct EngineUpgrader {
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

/// Attach an addon to a serivce subscription
async fn attach_addon<I>(
    rm: Arc<RuntimeManager>,
    plugins: Arc<PluginCollection>,
    pid: Pid,
    gid: GroupId,
    addon: EngineType,
    mode: SchedulingMode,
    tx_edges_replacement: I,
    rx_edges_replacement: I,
    indicator: Arc<DashSet<Pid>>,
) 
where
    I: IntoIterator<Item = ChannelDescriptor>,
{
    let mut group_engines = rm
        .engine_subscriptions
        .iter()
        .filter(|e| e.pid == pid && e.gid == gid)
        .map(|e| (*e.key(), e.value().clone()))
        .collect::<Vec<_>>();

    let guard = rm.inner.lock().unwrap();
    for (engine_id, info) in group_engines.iter() {
        let runtime = guard.runtimes.get(&info.rid).unwrap();
        runtime.request_suspend(*engine_id);
    }
    std::mem::drop(guard);

    let (mut subscription, _) = rm.service_subscriptions.remove(&(pid, gid)).unwrap().1;
    if subscription.addons.contains(&addon) {
        tracing::error!(
            "Addon engine {:?} not found in group (pid={:?}, gid={:?})",
            addon,
            pid,
            gid,
        );
        rm.global_resource_mgr.register_group_shutdown(pid);
        indicator.remove(&pid);
        return;
    }
    let mut engine_containers = Vec::with_capacity(group_engines.len());
    while group_engines.len() > 0 {
        let guard = rm.inner.lock().unwrap();
        group_engines.retain(|(eid, info)| {
            let runtime = guard.runtimes.get(&info.rid).unwrap();
            if let Some((_, result)) = runtime.suspended.remove(&eid) {
                if let SuspendResult::Engine(container) = result {
                    engine_containers.push((container, info.scheduling_mode));
                    rm.engine_subscriptions.remove(eid);
                }
                false
            } else {
                true
            }
        });
    }

    let mut detached_engines = HashMap::with_capacity(engine_containers.len());
    let mut detached_meta = HashMap::with_capacity(engine_containers.len());
    for (container, mode) in engine_containers {
        let engine_type = container.engine_type();
        let version = container.version();
        let mut engine = container.detach();
        engine.set_els();
        // DataPathNode may change for any engine
        // hence we need to flush the queues for all engines in the engine group
        if let Err(err) = engine.flush() {
            tracing::warn!(
                "Error in flushing engine (pid={:?}, gid={:?}, type={:?}), error: {:?}",
                pid,
                gid,
                engine_type,
                err,
            );
        };
        tracing::info!(
            "Engine (pid={:?}, gid={:?}, type={:?}) flushed",
            pid,
            gid,
            engine_type,
        );
        detached_engines.insert(engine_type, engine);
        detached_meta.insert(engine_type, (version, mode));
    }

    let node = match refactor_channels_attach_addon(
        &mut detached_engines,
        &mut subscription.graph,
        addon,
        tx_edges_replacement,
        rx_edges_replacement,
    ) {
        Ok(node) => node,
        Err(err) => {
            tracing::error!(
                "Fail to refactor data path channels in installing addon {:?} on group (pid={:?}, gid={:?}): {:?}",
                addon,
                pid,
                gid,
                err,
            );
            // discard the engine group
            // do not resubmit the engines
            rm.global_resource_mgr.register_group_shutdown(pid);
            indicator.remove(&pid);
            return;
        }
    };

    let mut containers_resubmit = Vec::with_capacity(detached_engines.len() + 1);
    let mut plugin = match plugins.engine_registry.get_mut(&addon) {
        Some(plugin) => {
            match plugin.value() {
                Plugin::Module(_) => {
                    tracing::error!("Engine type {:?} is not an addon", addon);
                    rm.global_resource_mgr.register_group_shutdown(pid);
                    return;
                },
                Plugin::Addon(addon_name) => {
                    plugins.addons.get_mut(addon_name).unwrap()
                },
            }
        },
        None => {
            tracing::error!("Addon for engine type {:?} not found", addon);
            rm.global_resource_mgr.register_group_shutdown(pid);
            indicator.remove(&pid);
            return;
        }
    };
    let version = plugin.version();
    let addon_engine = plugin
        .value_mut()
        .create_engine(addon, pid, node);
    
    match addon_engine {
        Ok(engine) => {
            let container = EngineContainer::new(engine, addon, version);
            containers_resubmit.push((container, mode))
        },
        Err(err) => {
            tracing::error!(
                "Failed to create addon engine {:?} for group (pid={:?}, gid={:?}), error: {:?}", 
                addon,
                pid,
                gid,
                err,
            );
            rm.global_resource_mgr.register_group_shutdown(pid);
            indicator.remove(&pid);
            return;
        }
    }
    tracing::info!(
        "Addon engine {:?} created, pid={:?}, gid={:?}",
        addon,
        pid,
        gid,
    );

    for (ty, engine) in detached_engines.into_iter() {
        let (version, mode) = detached_meta.remove(&ty).unwrap();
        let container = EngineContainer::new(engine, ty, version);
        containers_resubmit.push((container, mode));
    }
    subscription.addons.push(addon);

    rm.service_subscriptions.insert((pid, gid), (subscription, containers_resubmit.len()));
    for (container, mode) in containers_resubmit {
        tracing::info!(
            "Submitting engine {:?} (pid={:?}, gid={:?}) to runtime",
            container.engine_type(),
            pid,
            gid,
        );
        rm.submit(pid, gid, container, mode, false);
    }
    indicator.remove(&pid);
}

/// Detach an addon from a service subscription
async fn detach_addon<I>(
    rm: Arc<RuntimeManager>,
    pid: Pid,
    gid: GroupId,
    addon: EngineType,
    tx_edges_replacement: I,
    rx_edges_replacement: I,
    indicator: Arc<DashSet<Pid>>,
) 
where
    I: IntoIterator<Item = ChannelDescriptor>,
{
    let mut group_engines = rm
        .engine_subscriptions
        .iter()
        .filter(|e| e.pid == pid && e.gid == gid)
        .map(|e| (*e.key(), e.value().clone()))
        .collect::<Vec<_>>();

    let guard = rm.inner.lock().unwrap();
    for (engine_id, info) in group_engines.iter() {
        let runtime = guard.runtimes.get(&info.rid).unwrap();
        runtime.request_suspend(*engine_id);
    }
    std::mem::drop(guard);

    let (mut subscription, _) = rm.service_subscriptions.remove(&(pid, gid)).unwrap().1;
    
    if let Some(index) = subscription.addons.iter().position(|x| *x == addon) {
        subscription.addons.remove(index);
    } else {
        tracing::error!(
            "Addon engine {:?} not found in group (pid={:?}, gid={:?})",
            addon,
            pid,
            gid,
        );
        rm.global_resource_mgr.register_group_shutdown(pid);
        indicator.remove(&pid);
        return;
    }
    let mut engine_containers = Vec::with_capacity(group_engines.len());
    while group_engines.len() > 0 {
        let guard = rm.inner.lock().unwrap();
        group_engines.retain(|(eid, info)| {
            let runtime = guard.runtimes.get(&info.rid).unwrap();
            if let Some((_, result)) = runtime.suspended.remove(&eid) {
                if let SuspendResult::Engine(container) = result {
                    engine_containers.push((container, info.scheduling_mode));
                    rm.engine_subscriptions.remove(eid);
                }
                false
            } else {
                true
            }
        });
    }

    let mut detached_engines = HashMap::with_capacity(engine_containers.len());
    let mut detached_meta = HashMap::with_capacity(engine_containers.len());
    for (container, mode) in engine_containers {
        let engine_type = container.engine_type();
        let version = container.version();
        let mut engine = container.detach();
        engine.set_els();
        // DataPathNode may change for any engine
        // hence we need to flush the queues for all engines in the engine group
        if let Err(err) = engine.flush() {
            tracing::warn!(
                "Error in flushing engine (pid={:?}, gid={:?}, type={:?}), error: {:?}",
                pid,
                gid,
                engine_type,
                err,
            );
        };
        tracing::info!(
            "Engine (pid={:?}, gid={:?}, type={:?} flushed",
            pid,
            gid,
            engine_type,
        );
        detached_engines.insert(engine_type, engine);
        detached_meta.insert(engine_type, (version, mode));
    }

    let result = refactor_channels_detach_addon(
        &mut detached_engines, 
        &mut subscription.graph, 
        addon, 
        tx_edges_replacement, 
        rx_edges_replacement,
    );
    if let Err(err) = result {
        tracing::error!(
            "Failed to refactor data path channels in uninstall addon {:?} for group (pid={:?}, gid={:?}), error: {:?}", 
            addon,
            pid,
            gid,
            err,
        );
        rm.global_resource_mgr.register_group_shutdown(pid);
        indicator.remove(&pid);
        return;
    }

    let mut containers_resubmit = Vec::with_capacity(detached_engines.len());
    for (ty, engine) in detached_engines.into_iter() {
        let (version, mode) = detached_meta.remove(&ty).unwrap();
        let container = EngineContainer::new(engine, ty, version);
        containers_resubmit.push((container, mode));
    }

    rm.service_subscriptions.insert((pid, gid), (subscription, containers_resubmit.len()));
    for (container, mode) in containers_resubmit {
        tracing::info!(
            "Submitting engine {:?} (pid={:?}, gid={:?}) to runtime",
            container.engine_type(),
            pid,
            gid,
        );
        rm.submit(pid, gid, container, mode, false);
    } 
    indicator.remove(&pid);
}

/// Upgrade the engines of a client process
/// Arguments:
/// * to_upgrade: eninges to be upgraded
/// * to_suspend: engines that do not need upgrade
///     but need to be suspended from runtimes
///     in order to properly flush data and command queues
/// * flush: whether to flush the queues
///     of each engine to be upgraded
/// Note:
/// If all engines in an engine group (service subscription)
/// is shutdown, to be upgraded, or to be suspended,
/// then the new engines will submit in a new group.
/// Otherwise, they will submit to the original engine group
async fn upgrade_client(
    rm: Arc<RuntimeManager>,
    plugins: Arc<PluginCollection>,
    pid: Pid,
    mut to_upgrade: Vec<(EngineId, EngineInfo)>,
    mut to_suspend: Vec<(EngineId, EngineInfo)>,
    flush: bool,
    indicator: Arc<DashSet<Pid>>,
) {
    let guard = rm.inner.lock().unwrap();
    for (engine_id, info) in to_upgrade.iter().chain(to_suspend.iter()) {
        let runtime = guard.runtimes.get(&info.rid).unwrap();
        runtime.request_suspend(*engine_id);
    }
    std::mem::drop(guard);

    // EngineContainers suspended from runtimes, awaiting for upgrade
    let mut containers_to_upgrade = HashMap::new();
    // EngineContainers for engines in the same engine groups
    // that do not need update, but need to suspend from runtimes,
    let mut containers_suspended = HashMap::new();
    while to_upgrade.len() > 0 || to_suspend.len() > 0 {
        let guard = rm.inner.lock().unwrap();
        to_upgrade.retain(|(eid, info)| {
            let runtime = guard.runtimes.get(&info.rid).unwrap();
            if let Some((_, result)) = runtime.suspended.remove(eid) {
                if let SuspendResult::Engine(container) = result {
                    let group = containers_to_upgrade.entry(info.gid).or_insert_with(Vec::new);
                    group.push((container, info.scheduling_mode));
                    // remove the engine from subscriptions
                    // but we don't decrease the reference count
                    // of the corresponding service subscription
                    rm.engine_subscriptions.remove(eid);
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
        to_suspend.retain(|(eid, info)| {
            let runtime = guard.runtimes.get(&info.rid).unwrap();
            if let Some((_, result)) = runtime.suspended.remove(eid) {
                if let SuspendResult::Engine(container) = result {
                    let group = containers_suspended.entry(info.gid).or_insert_with(Vec::new);
                    group.push((container, info.scheduling_mode));
                    rm.engine_subscriptions.remove(eid);
                }
                false
            } else {
                true
            }
        });
    }
    let subscribed_groups = containers_to_upgrade
        .keys()
        .chain(containers_suspended.keys())
        .map(|x| *x)
        .collect::<Vec<_>>();

    // Each engine to upgrade's local state
    let mut local_states = HashMap::new();
    // Shared storage for each engine group
    let mut shared_storage = HashMap::new();
    // Global resources that are shared by all engines
    let mut global_resource = rm
        .global_resource_mgr
        .resource
        .entry(pid)
        .or_insert_with(ResourceCollection::new);
    // Decompose the engines to upgrade
    for (gid, containers) in containers_to_upgrade {
        let shared = shared_storage.entry(gid).or_insert_with(SharedStorage::new);
        for (container, mode) in containers {
            let prev_version = container.version();
            let engine_type = container.engine_type();
            let mut engine = container.detach();
            engine.set_els();
            if flush {
                if let Err(err) = engine.flush() {
                    tracing::warn!(
                        "Error in flushing engine (pid={:?}, gid={:?}, type={:?}), error: {:?}",
                        pid,
                        gid,
                        engine_type,
                        err,
                    );
                };
            }
            let (state, node) = engine.decompose(shared, global_resource.value_mut());
            tracing::info!(
                "Engine (pid={:?}, gid={:?}, type={:?}) decomposed",
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
            entry.insert(engine_type, dumped);
        }
    }

    // Handle engines that will be resubmit in a new group
    for gid in subscribed_groups {
        let mut subscription_guard = rm.service_subscriptions.get_mut(&(pid, gid)).unwrap();
        let (subscription, _) = subscription_guard.value_mut();
        let mut service = plugins.service_registry.get_mut(&subscription.service).unwrap();
        // redirects all `ServiceType` in `subscription`
        // to point to the new &'static str
        subscription.service = *service.key();

        let mut resubmit = true;
        let mut containers_resubmit = if let Some(containers) = containers_suspended.remove(&gid) {
            containers
        } else {
            Vec::with_capacity(service.engines.len())
        };
        let mut resubmit_count = containers_resubmit.len();

        if let Some(mut engine_group) = local_states.remove(&gid) {
            resubmit_count += engine_group.len();
            let mut shared = shared_storage.remove(&gid).unwrap();
            for subscribed_engine_ty in service.engines.iter_mut().chain(subscription.addons.iter_mut()) {
                if let Some(dumped) = engine_group.remove(&subscribed_engine_ty) {
                    let plugin = plugins.engine_registry.get(&subscribed_engine_ty).unwrap();

                    // redirects all `EngineType` in `subscription`
                    // to point to &'static str in the new shared library
                    // otherwise, these `EngineType` become invalid after old library is unloaded
                    let engine_ty_relocated = *plugin.key();
                    if let Plugin::Addon(_) = plugin.value() {
                        // `EngineType` in `ServiceRegistry` should already been relocated
                        // but not for the addons in `ServiceSubscription`
                        *subscribed_engine_ty = engine_ty_relocated;
                    }
                    if let Some(tx_inputs) = subscription.graph.tx_inputs.remove(&engine_ty_relocated) {
                        for (peer, index) in tx_inputs.iter() {
                            subscription.graph.tx_outputs.get_mut(peer).unwrap()[*index].0 = engine_ty_relocated;
                        }
                        subscription.graph.tx_inputs.insert(engine_ty_relocated, tx_inputs);
                    } 
                    if let Some(tx_outputs) =  subscription.graph.tx_outputs.remove(&engine_ty_relocated) {
                        for (peer, index) in tx_outputs.iter() {
                            subscription.graph.tx_inputs.get_mut(peer).unwrap()[*index].0 = engine_ty_relocated;
                        }
                        subscription.graph.tx_outputs.insert(engine_ty_relocated, tx_outputs);
                    }
                    if let Some(rx_inputs) = subscription.graph.rx_inputs.remove(&engine_ty_relocated) {
                        for (peer, index) in rx_inputs.iter() {
                            subscription.graph.rx_outputs.get_mut(peer).unwrap()[*index].0 = engine_ty_relocated;
                        }
                        subscription.graph.rx_inputs.insert(engine_ty_relocated, rx_inputs);
                    } 
                    if let Some(rx_outputs) =  subscription.graph.rx_outputs.remove(&engine_ty_relocated) {
                        for (peer, index) in rx_outputs.iter() {
                            subscription.graph.rx_inputs.get_mut(peer).unwrap()[*index].0 = engine_ty_relocated;
                        }
                        subscription.graph.rx_outputs.insert(engine_ty_relocated, rx_outputs);
                    }

                    let (engine, new_version) = match plugin.value() {
                        Plugin::Module(module_name) => {
                            let mut module = plugins.modules.get_mut(module_name).unwrap();
                            let new_version = module.version();
                            // resotre engine
                            let engine = module.restore_engine(
                                *subscribed_engine_ty,
                                dumped.local_states,
                                &mut shared,
                                global_resource.value_mut(),
                                dumped.node,
                                &plugins.modules,
                                dumped.prev_version,
                            );
                            (engine, new_version)
                        }
                        Plugin::Addon(addon_name) => {
                            let mut addon = plugins.addons.get_mut(addon_name).unwrap();
                            let new_version = addon.version();
                            let engine = addon.restore_engine(
                                *subscribed_engine_ty,
                                dumped.local_states,
                                dumped.node,
                                dumped.prev_version,
                            );
                            (engine, new_version)
                        }
                    };
                    match engine {
                        Ok(engine) => {
                            let container =
                                EngineContainer::new(engine, *subscribed_engine_ty, new_version);
                            containers_resubmit.push((container, dumped.mode));
                            tracing::info!(
                                "Engine (pid={:?}, gid={:?}, type={:?}) restored",
                                pid,
                                gid,
                                subscribed_engine_ty,
                            );
                        }
                        Err(err) => {
                            tracing::error!(
                                "Failed to restore engine (pid={:?}, gid={:?}, type={:?}), error: {:?}",
                                pid,
                                gid,
                                subscribed_engine_ty,
                                err,
                            );
                            resubmit = false;
                            break;
                        }
                    }
                }
            }
        }

        std::mem::drop(subscription_guard);
        if resubmit {
            for (container, mode) in containers_resubmit {
                tracing::info!(
                    "Submitting engine {:?} (pid={:?}, gid={:?}) to runtime",
                    container.engine_type(),
                    pid,
                    gid,
                );
                rm.submit(pid, gid, container, mode, false);
            }
        } else {
            // error has occurred, rollback
            // cancel all pending submission 
            let removed =
                rm.service_subscriptions
                    .remove_if_mut(&(pid, gid), |_, (_, cnt)| {
                        *cnt -= resubmit_count;
                        *cnt == 0
            });
            if removed.is_some() {
                rm.global_resource_mgr.register_group_shutdown(pid);
            }
        }
    }

    indicator.remove(&pid);
    if indicator.is_empty() {
        plugins.upgrade_cleanup();
    }
}

impl EngineUpgrader {
    pub(crate) fn new(rm: Arc<RuntimeManager>, plugins: Arc<PluginCollection>) -> Self {
        let pool = ThreadPoolBuilder::new().pool_size(1).create().unwrap();
        EngineUpgrader {
            runtime_manager: rm,
            plugins: plugins,
            executor: pool,
            upgrade_indicator: Arc::new(DashSet::new()),
        }
    }

    /// Attach an addon to a service subscription
    pub(crate) fn attach_addon<I>(
        &mut self,
        pid: Pid,
        gid: GroupId,
        addon: EngineType,
        mode: SchedulingMode,
        tx_edges_replacement: I,
        rx_edges_replacement: I,
    ) -> anyhow::Result<()>
    where
        I: IntoIterator<Item = ChannelDescriptor> + Send + 'static,
    {
        if self.upgrade_indicator.contains(&pid) {
            bail!("there is already an ongoing upgrade for client pid={:?}", pid)
        }
        self.upgrade_indicator.insert(pid);
        let fut = attach_addon(
            self.runtime_manager.clone(), 
            self.plugins.clone(),
            pid, 
            gid, 
            addon, 
            mode, 
            tx_edges_replacement, 
            rx_edges_replacement, 
            Arc::clone(&self.upgrade_indicator),
        );
        self.executor.spawn_ok(fut);
        Ok(())
    }

    /// Detach an addon from a service subscription
    pub(crate) fn detach_addon<I>(
        &mut self,
        pid: Pid,
        gid: GroupId,
        addon: EngineType,
        tx_edges_replacement: I,
        rx_edges_replacement: I,
    ) -> anyhow::Result<()>
    where
        I: IntoIterator<Item = ChannelDescriptor> + Send + 'static,
    {   
        if self.upgrade_indicator.contains(&pid) {
            bail!("there is already an ongoing upgrade for client pid={:?}", pid)
        }
        self.upgrade_indicator.insert(pid);
        let fut = detach_addon(
            self.runtime_manager.clone(), 
            pid, 
            gid, 
            addon, 
            tx_edges_replacement, 
            rx_edges_replacement, 
            Arc::clone(&self.upgrade_indicator),
        );
        self.executor.spawn_ok(fut);
        Ok(())
    }
    /// Live upgrade existing clients
    /// Arguments:
    /// * engine_types: engines that need to be upgraded
    /// * flush: whether to flush the queues for the engines to be upgraded
    /// * detach_group: whether to suspend/detach all engines in each engine group,
    ///     even the engine does not need upgrade, this is generally required to flush queues 
    pub(crate) fn upgrade(
        &mut self,
        engine_types: HashSet<EngineType>,
        flush: bool,
        detach_group: bool,
    ) -> anyhow::Result<()> {
        if !self.upgrade_indicator.is_empty() {
            bail!("there is already an ongoing upgrade")
        }

        if flush && !detach_group {
            tracing::warn!("Flush queues but not detaching all engines within each group during upgrade")
        }

        // engines that need to be upgraded
        let mut engines_to_upgrade = HashMap::new();
        // other engines that are in the same group
        // as the engines to be upgraded
        let mut engines_to_detach = HashMap::new();

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
            client.push((*engine.key(), *engine.value()));
            groups_to_upgrade.insert((engine.pid, engine.gid));
        }

        if detach_group {
            for engine in self
                .runtime_manager
                .engine_subscriptions
                .iter()
                .filter(|e| {
                    !engine_types.contains(&e.engine_type)
                        && groups_to_upgrade.contains(&(e.pid, e.gid))
                })
            {
                let client = engines_to_detach
                    .entry(engine.pid)
                    .or_insert_with(Vec::new);
                client.push((*engine.key(), engine.value().clone()));
            }
        }

        for (pid, to_upgrade) in engines_to_upgrade {
            let to_detach = if let Some(engines) = engines_to_detach.remove(&pid) {
                engines
            } else { Vec::new() };
            let rm = Arc::clone(&self.runtime_manager);
            let plugins = Arc::clone(&self.plugins);
            let fut = upgrade_client(
                rm,
                plugins,
                pid,
                to_upgrade,
                to_detach,
                flush,
                Arc::clone(&self.upgrade_indicator),
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
