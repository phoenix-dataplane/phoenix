use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use anyhow::bail;
use dashmap::DashSet;
use futures::executor::{ThreadPool, ThreadPoolBuilder};
use nix::unistd::Pid;
use semver::Version;
  
use interface::engine::SchedulingMode;

use super::datapath::{refactor_channels_attach_addon, refactor_channels_detach_addon};
use super::datapath::{ChannelDescriptor, DataPathNode};
use super::manager::{EngineId, EngineInfo, SubscriptionId, RuntimeManager, RuntimeId};
use super::runtime::SuspendResult;
use super::{EngineContainer, EngineType};
use crate::engine::group::GroupId;
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
    rid: RuntimeId,
    gid: GroupId,
    mode: SchedulingMode,
}

/// Attach an addon to a serivce subscription
async fn attach_addon<I>(
    rm: Arc<RuntimeManager>,
    plugins: Arc<PluginCollection>,
    pid: Pid,
    sid: SubscriptionId,
    addon: EngineType,
    mode: SchedulingMode,
    tx_edges_replacement: I,
    rx_edges_replacement: I,
    group: HashSet<EngineType>,
    indicator: Arc<DashSet<Pid>>,
) where
    I: IntoIterator<Item = ChannelDescriptor>,
{
    let mut subscription_engines = rm
        .engine_subscriptions
        .iter()
        .filter(|e| e.pid == pid && e.sid == sid)
        .map(|e| (*e.key(), e.value().clone()))
        .collect::<Vec<_>>();

    let guard = rm.inner.lock().unwrap();
    for (engine_id, info) in subscription_engines.iter() {
        let runtime = guard.runtimes.get(&info.rid).unwrap();
        runtime.request_suspend(*engine_id);
    }
    std::mem::drop(guard);

    let (mut subscription, _) = rm.service_subscriptions.remove(&(pid, sid)).unwrap().1;
    if subscription.addons.contains(&addon) {
        tracing::error!(
            "Addon engine {:?} not found in group (pid={:?}, sid={:?})",
            addon,
            pid,
            sid,
        );
        rm.global_resource_mgr.register_group_shutdown(pid);
        indicator.remove(&pid);
        return;
    }
    let mut engine_containers = Vec::with_capacity(subscription_engines.len());
    while subscription_engines.len() > 0 {
        let guard = rm.inner.lock().unwrap();
        subscription_engines.retain(|(eid, info)| {
            let runtime = guard.runtimes.get(&info.rid).unwrap();
            if let Some((_, result)) = runtime.suspended.remove(&eid) {
                if let SuspendResult::Engine(container) = result {
                    engine_containers.push((container, *info));
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
    for (container, info) in engine_containers {
        let engine_type = info.engine_type;
        let version = container.version();
        let engine = container.detach();
        detached_engines.insert(engine_type, engine);
        detached_meta.insert(engine_type, (info, version));
    }

    let dataflow_order = subscription.graph.topological_order();
    for (engine_type, _) in dataflow_order.into_iter() {
        let engine = detached_engines.get_mut(&engine_type).unwrap();
        engine.set_els();
        // DataPathNode may change for any engine
        // hence we need to flush the queues for all engines in the engine group
        if let Err(err) = engine.flush() {
            tracing::warn!(
                "Error in flushing engine (pid={:?}, sid={:?}, type={:?}), error: {:?}",
                pid,
                sid,
                engine_type,
                err,
            );
        };
        tracing::info!(
            "Engine (pid={:?}, sid={:?}, type={:?}) flushed",
            pid,
            sid,
            engine_type,
        );
    }

    let node = match refactor_channels_attach_addon(
        &mut detached_engines,
        &mut subscription.graph,
        addon,
        tx_edges_replacement,
        rx_edges_replacement,
        &group,
    ) {
        Ok(node) => node,
        Err(err) => {
            tracing::error!(
                "Fail to refactor data path channels in installing addon {:?} on group (pid={:?}, sid={:?}): {:?}",
                addon,
                pid,
                sid,
                err,
            );
            // discard the engine group
            // do not resubmit the engines
            rm.global_resource_mgr.register_group_shutdown(pid);
            indicator.remove(&pid);
            return;
        }
    };

    let mut containers_resubmit = HashMap::new();
    let mut plugin = match plugins.engine_registry.get_mut(&addon) {
        Some(plugin) => match plugin.value() {
            Plugin::Module(_) => {
                tracing::error!("Engine type {:?} is not an addon", addon);
                rm.global_resource_mgr.register_group_shutdown(pid);
                return;
            }
            Plugin::Addon(addon_name) => plugins.addons.get_mut(addon_name).unwrap(),
        },
        None => {
            tracing::error!("Addon for engine type {:?} not found", addon);
            rm.global_resource_mgr.register_group_shutdown(pid);
            indicator.remove(&pid);
            return;
        }
    };
    let version = plugin.version();
    let addon_engine = plugin.value_mut().create_engine(addon, pid, node);

    match addon_engine {
        Ok(engine) => {
            let container = EngineContainer::new(engine, addon, version);
            let (gid, rid) = if group.is_empty() {
                let peer = *group.iter().next().unwrap();
                let (info, _) = detached_meta.get(&peer).unwrap();
                (info.gid, Some(info.rid))
            } else {
                let gid = GroupId(rm.scheduling_group_counter.fetch_add(1, Ordering::Relaxed));
                (gid, None)
            };
            let entry = containers_resubmit
                .entry(gid)
                .or_insert((Vec::new(), mode, rid));
            entry.0.push(container)
        }
        Err(err) => {
            tracing::error!(
                "Failed to create addon engine {:?} for group (pid={:?}, sid={:?}), error: {:?}",
                addon,
                pid,
                sid,
                err,
            );
            rm.global_resource_mgr.register_group_shutdown(pid);
            indicator.remove(&pid);
            return;
        }
    }
    tracing::info!(
        "Addon engine {:?} created, pid={:?}, sid={:?}",
        addon,
        pid,
        sid,
    );

    for (ty, engine) in detached_engines.into_iter() {
        let (info, version) = detached_meta.remove(&ty).unwrap();
        let container = EngineContainer::new(engine, ty, version);
        let gid = info.gid;
        let rid = info.rid;
        let entry = containers_resubmit
            .entry(gid)
            .or_insert((Vec::new(), info.scheduling_mode, Some(rid)));
        entry.0.push(container);
    }
    subscription.addons.push(addon);

    let engines_count = containers_resubmit
        .iter()
        .map(|(_, (engines, ..))| engines.len())
        .sum();
    rm.service_subscriptions
        .insert((pid, sid), (subscription, engines_count));
    for (group_id, (containers, mode, rid)) in containers_resubmit {
        if let Some(rid) = rid {
            rm.attach_to_group(pid, sid, group_id, rid, containers, mode);
        } else {
            rm.submit_group(pid, sid, containers, mode);
        }
    }
    indicator.remove(&pid);
}

/// Detach an addon from a service subscription
async fn detach_addon<I>(
    rm: Arc<RuntimeManager>,
    pid: Pid,
    sid: SubscriptionId,
    addon: EngineType,
    tx_edges_replacement: I,
    rx_edges_replacement: I,
    indicator: Arc<DashSet<Pid>>,
) where
    I: IntoIterator<Item = ChannelDescriptor>,
{
    let mut subscription_engines = rm
        .engine_subscriptions
        .iter()
        .filter(|e| e.pid == pid && e.sid == sid)
        .map(|e| (*e.key(), e.value().clone()))
        .collect::<Vec<_>>();

    let guard = rm.inner.lock().unwrap();
    for (engine_id, info) in subscription_engines.iter() {
        let runtime = guard.runtimes.get(&info.rid).unwrap();
        runtime.request_suspend(*engine_id);
    }
    std::mem::drop(guard);

    let (mut subscription, _) = rm.service_subscriptions.remove(&(pid, sid)).unwrap().1;

    if let Some(index) = subscription.addons.iter().position(|x| *x == addon) {
        subscription.addons.remove(index);
    } else {
        tracing::error!(
            "Addon engine {:?} not found in group (pid={:?}, gid={:?})",
            addon,
            pid,
            sid,
        );
        rm.global_resource_mgr.register_group_shutdown(pid);
        indicator.remove(&pid);
        return;
    }
    let mut engine_containers = Vec::with_capacity(subscription_engines.len());
    while subscription_engines.len() > 0 {
        let guard = rm.inner.lock().unwrap();
        subscription_engines.retain(|(eid, info)| {
            let runtime = guard.runtimes.get(&info.rid).unwrap();
            if let Some((_, result)) = runtime.suspended.remove(&eid) {
                if let SuspendResult::Engine(container) = result {
                    engine_containers.push((container, *info));
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
    for (container, info) in engine_containers {
        let engine_type = info.engine_type;
        let version = container.version();
        let engine = container.detach();
        detached_engines.insert(engine_type, (engine, info.gid));
        detached_meta.insert(engine_type, (info, version));
    }

    let dataflow_order = subscription.graph.topological_order();
    for (engine_type, _) in dataflow_order.into_iter() {
        let (engine, _) = detached_engines.get_mut(&engine_type).unwrap();
        engine.set_els();
        // DataPathNode may change for any engine
        // hence we need to flush the queues for all engines in the engine group
        if let Err(err) = engine.flush() {
            tracing::warn!(
                "Error in flushing engine (pid={:?}, sid={:?}, type={:?}), error: {:?}",
                pid,
                sid,
                engine_type,
                err,
            );
        };
        tracing::info!(
            "Engine (pid={:?}, sid={:?}, type={:?}) flushed",
            pid,
            sid,
            engine_type,
        );
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
            "Failed to refactor data path channels in uninstall addon {:?} for group (pid={:?}, sid={:?}), error: {:?}", 
            addon,
            pid,
            sid,
            err,
        );
        rm.global_resource_mgr.register_group_shutdown(pid);
        indicator.remove(&pid);
        return;
    }

    let mut containers_resubmit = HashMap::new();
    for (ty, (engine, _)) in detached_engines.into_iter() {
        let (info, version) = detached_meta.remove(&ty).unwrap();
        let container = EngineContainer::new(engine, ty, version);
        let gid = info.gid;
        let rid = info.rid;
        let entry = containers_resubmit
            .entry(gid)
            .or_insert((Vec::new(), info.scheduling_mode, rid));
        entry.0.push(container);
    }
    subscription.addons.push(addon);

    let engines_count = containers_resubmit
        .iter()
        .map(|(_, (engines, ..))| engines.len())
        .sum();
    rm.service_subscriptions
        .insert((pid, sid), (subscription, engines_count));
    for (group_id, (containers, mode, rid)) in containers_resubmit {
        rm.attach_to_group(pid, sid, group_id, rid, containers, mode);
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
    let mut engines_to_upgrade = HashMap::new();
    // EngineContainers for engines in the same engine subscription
    // that do not need update, but need to suspend from runtimes,
    let mut containers_suspended = HashMap::new();
    while to_upgrade.len() > 0 || to_suspend.len() > 0 {
        let guard = rm.inner.lock().unwrap();
        to_upgrade.retain(|(eid, info)| {
            let runtime = guard.runtimes.get(&info.rid).unwrap();
            if let Some((_, result)) = runtime.suspended.remove(eid) {
                if let SuspendResult::Engine(container) = result {
                    let subscription = engines_to_upgrade
                        .entry(info.sid)
                        .or_insert_with(HashMap::new);
                    let engine_type = container.engine_type();
                    let version = container.version();
                    let engine = container.detach();
                    subscription.insert(engine_type, (engine, *info, version));
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
                    let subscription = containers_suspended
                        .entry(info.sid)
                        .or_insert_with(Vec::new);
                    subscription.push((container, *info));
                    rm.engine_subscriptions.remove(eid);
                }
                false
            } else {
                true
            }
        });
    }
    let subscribed_groups = engines_to_upgrade
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

    if flush {
        for (sid, engines) in engines_to_upgrade.iter_mut() {
            let mut subscription_guard = rm.service_subscriptions.get_mut(&(pid, *sid)).unwrap();
            let (subscription, _) = subscription_guard.value_mut();
            let dataflow_order = subscription.graph.topological_order();
            for (engine_type, _) in dataflow_order.into_iter() {
                let (engine, ..) = engines.get_mut(&engine_type).unwrap();
                engine.set_els();
                // DataPathNode may change for any engine
                // hence we need to flush the queues for all engines in the engine group
                if let Err(err) = engine.flush() {
                    tracing::warn!(
                        "Error in flushing engine (pid={:?}, sid={:?}, type={:?}), error: {:?}",
                        pid,
                        sid,
                        engine_type,
                        err,
                    );
                };
                tracing::info!(
                    "Engine (pid={:?}, sid={:?}, type={:?}) flushed",
                    pid,
                    sid,
                    engine_type,
                );
            }
        }
    }

    // Decompose the engines to upgrade
    for (sid, engines) in engines_to_upgrade {
        let shared = shared_storage.entry(sid).or_insert_with(SharedStorage::new);
        for (engine_type, (engine, info, prev_version)) in engines {
            engine.set_els();
            let (state, node) = engine.decompose(shared, global_resource.value_mut());
            tracing::info!(
                "Engine (pid={:?}, sid={:?}, type={:?}) decomposed",
                pid,
                sid,
                engine_type,
            );
            let entry = local_states.entry(sid).or_insert_with(HashMap::new);
            let dumped = EngineDumped {
                local_states: state,
                node,
                prev_version,
                rid: info.rid,
                gid: info.gid,
                mode: info.scheduling_mode
            };
            entry.insert(engine_type, dumped);
        }
    }

    // Handle engines that will be resubmit in a new group
    for sid in subscribed_groups {
        let mut subscription_guard = rm.service_subscriptions.get_mut(&(pid, sid)).unwrap();
        let (subscription, _) = subscription_guard.value_mut();
        let mut service = plugins
            .service_registry
            .get_mut(&subscription.service)
            .unwrap();
        // redirects all `ServiceType` in `subscription`
        // to point to the new &'static str
        subscription.service = *service.key();

        let mut resubmit = true;
        let mut containers_resubmit = HashMap::new(); 
        let mut resubmit_count = 0;
        if let Some(containers) = containers_suspended.remove(&sid) {
            for (container, info) in containers {
                let gid = info.gid;
                let rid = info.rid;
                let entry = containers_resubmit
                    .entry(gid)
                    .or_insert((Vec::new(), info.scheduling_mode, rid));
                entry.0.push(container); 
                resubmit_count += 1;
            }
        }

        if let Some(mut engine_group) = local_states.remove(&sid) {
            resubmit_count += engine_group.len();
            let mut shared = shared_storage.remove(&sid).unwrap();
            for subscribed_engine_ty in service
                .engines
                .iter_mut()
                .chain(subscription.addons.iter_mut())
            {
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
                    if let Some(tx_inputs) =
                        subscription.graph.tx_inputs.remove(&engine_ty_relocated)
                    {
                        for (peer, index) in tx_inputs.iter() {
                            subscription.graph.tx_outputs.get_mut(peer).unwrap()[*index].0 =
                                engine_ty_relocated;
                        }
                        subscription
                            .graph
                            .tx_inputs
                            .insert(engine_ty_relocated, tx_inputs);
                    }
                    if let Some(tx_outputs) =
                        subscription.graph.tx_outputs.remove(&engine_ty_relocated)
                    {
                        for (peer, index) in tx_outputs.iter() {
                            subscription.graph.tx_inputs.get_mut(peer).unwrap()[*index].0 =
                                engine_ty_relocated;
                        }
                        subscription
                            .graph
                            .tx_outputs
                            .insert(engine_ty_relocated, tx_outputs);
                    }
                    if let Some(rx_inputs) =
                        subscription.graph.rx_inputs.remove(&engine_ty_relocated)
                    {
                        for (peer, index) in rx_inputs.iter() {
                            subscription.graph.rx_outputs.get_mut(peer).unwrap()[*index].0 =
                                engine_ty_relocated;
                        }
                        subscription
                            .graph
                            .rx_inputs
                            .insert(engine_ty_relocated, rx_inputs);
                    }
                    if let Some(rx_outputs) =
                        subscription.graph.rx_outputs.remove(&engine_ty_relocated)
                    {
                        for (peer, index) in rx_outputs.iter() {
                            subscription.graph.rx_inputs.get_mut(peer).unwrap()[*index].0 =
                                engine_ty_relocated;
                        }
                        subscription
                            .graph
                            .rx_outputs
                            .insert(engine_ty_relocated, rx_outputs);
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
                            let gid = dumped.gid;
                            let rid = dumped.rid;
                            let entry = containers_resubmit
                                .entry(gid)
                                .or_insert((Vec::new(), dumped.mode, rid));
                            entry.0.push(container); 
                            tracing::info!(
                                "Engine (pid={:?}, sid={:?}, type={:?}) restored",
                                pid,
                                sid,
                                subscribed_engine_ty,
                            );
                        }
                        Err(err) => {
                            tracing::error!(
                                "Failed to restore engine (pid={:?}, sid={:?}, type={:?}), error: {:?}",
                                pid,
                                sid,
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
            for (group_id, (containers, mode, rid)) in containers_resubmit {
                rm.attach_to_group(pid, sid, group_id, rid, containers, mode);
            }
        } else {
            // error has occurred, rollback
            // cancel all pending submission
            let removed = rm
                .service_subscriptions
                .remove_if_mut(&(pid, sid), |_, (_, cnt)| {
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
            plugins,
            executor: pool,
            upgrade_indicator: Arc::new(DashSet::new()),
        }
    }

    /// Attach an addon to a service subscription
    pub(crate) fn attach_addon<I>(
        &mut self,
        pid: Pid,
        gid: SubscriptionId,
        addon: EngineType,
        mode: SchedulingMode,
        tx_edges_replacement: I,
        rx_edges_replacement: I,
        group: HashSet<EngineType>,
    ) -> anyhow::Result<()>
    where
        I: IntoIterator<Item = ChannelDescriptor> + Send + 'static,
    {
        if self.upgrade_indicator.contains(&pid) {
            bail!(
                "there is already an ongoing upgrade for client pid={:?}",
                pid
            )
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
            group,
            Arc::clone(&self.upgrade_indicator),
        );
        self.executor.spawn_ok(fut);
        Ok(())
    }

    /// Detach an addon from a service subscription
    pub(crate) fn detach_addon<I>(
        &mut self,
        pid: Pid,
        gid: SubscriptionId,
        addon: EngineType,
        tx_edges_replacement: I,
        rx_edges_replacement: I,
    ) -> anyhow::Result<()>
    where
        I: IntoIterator<Item = ChannelDescriptor> + Send + 'static,
    {
        if self.upgrade_indicator.contains(&pid) {
            bail!(
                "there is already an ongoing upgrade for client pid={:?}",
                pid
            )
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
            tracing::warn!(
                "Flush queues but not detaching all engines within each group during upgrade"
            )
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
            groups_to_upgrade.insert((engine.pid, engine.sid));
        }

        if detach_group {
            for engine in self
                .runtime_manager
                .engine_subscriptions
                .iter()
                .filter(|e| {
                    !engine_types.contains(&e.engine_type)
                        && groups_to_upgrade.contains(&(e.pid, e.sid))
                })
            {
                let client = engines_to_detach.entry(engine.pid).or_insert_with(Vec::new);
                client.push((*engine.key(), engine.value().clone()));
            }
        }

        for (pid, to_upgrade) in engines_to_upgrade {
            let to_detach = if let Some(engines) = engines_to_detach.remove(&pid) {
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
