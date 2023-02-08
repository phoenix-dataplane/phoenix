use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::bail;
use crc32fast::Hasher as Crc32Hasher;
use dashmap::DashMap;
use itertools::Itertools;
use uapi::engine::SchedulingMode;

use ipc::control::PluginDescriptor;

use crate::addon::PhoenixAddon;
use crate::config::LinkerConfig;
use crate::dependency::EngineGraph;
use crate::engine::datapath::graph::ChannelDescriptor;
use crate::engine::group::GroupUnionFind;
use crate::engine::EngineType;
use crate::linker::Linker;
use crate::module::PhoenixModule;
use crate::module::Service;
use crate::plugin::{Plugin, PluginName};

pub(crate) struct ServiceRegistry {
    pub(crate) engines: Vec<EngineType>,
    pub(crate) tx_channels: Vec<ChannelDescriptor>,
    pub(crate) rx_channels: Vec<ChannelDescriptor>,
    pub(crate) scheduling_groups: GroupUnionFind,
}

// COMMENT(wyj): drop order matters
pub struct PluginManager {
    default_prefix: PathBuf,
    pub(crate) modules: DashMap<String, Box<dyn PhoenixModule>>,
    pub(crate) addons: DashMap<String, Box<dyn PhoenixAddon>>,
    pub(crate) engine_registry: DashMap<EngineType, (PluginName, Option<SchedulingMode>)>,
    pub(crate) service_registry: DashMap<Service, ServiceRegistry>,
    dependency_graph: Mutex<EngineGraph>,
    /// signature of different scheduling groups
    /// that service engines (addon engines are excluded) will create
    scheduling_group_signatures: Mutex<HashMap<u32, String>>,

    plugins: ManuallyDrop<DashMap<PluginDescriptor, Plugin>>,
    rt_linker: Mutex<Linker>,
}

impl PluginManager {
    /// Returns an empty PluginManager.
    pub fn new<P: AsRef<Path>>(prefix: P, linker_config: &LinkerConfig) -> anyhow::Result<Self> {
        let default_prefix = prefix.as_ref().to_path_buf();
        let rt_linker_workdir = if linker_config.workdir.is_absolute() {
            linker_config.workdir.clone()
        } else {
            default_prefix.join(linker_config.workdir.clone())
        };
        Ok(PluginManager {
            default_prefix: default_prefix.clone(),
            modules: DashMap::new(),
            addons: DashMap::new(),
            engine_registry: DashMap::new(),
            service_registry: DashMap::new(),
            dependency_graph: Mutex::new(EngineGraph::new()),
            scheduling_group_signatures: Mutex::new(HashMap::new()),
            plugins: ManuallyDrop::new(DashMap::new()),
            rt_linker: Mutex::new(Linker::new(rt_linker_workdir)?),
        })
    }

    // Returns the lib_path and dep_path of a plugin
    fn get_plugin_path(&self, desc: &PluginDescriptor) -> (PathBuf, PathBuf) {
        let lib_path = if desc.lib_path.is_absolute() {
            desc.lib_path.to_path_buf()
        } else {
            self.default_prefix.join(desc.lib_path.clone())
        };

        if let Some(dep_path) = desc.dep_path.clone() {
            (lib_path, dep_path)
        } else {
            (lib_path.clone(), lib_path.with_extension("d"))
        }
    }

    pub fn load_or_upgrade_addon(&self, addon: &PluginDescriptor) -> anyhow::Result<()> {
        // Get the library path and its dep file path
        let (lib_path, dep_path) = self.get_plugin_path(&addon);

        // RT linker load the rlib and its all transitive dependencies
        let linked = {
            let mut linker = self.rt_linker.lock().unwrap();
            linker.load_archive(lib_path, dep_path)?
        };

        // Replace the old plugin or create the new plugin
        let new_plug = match self.plugins.remove(&addon) {
            Some((_, old_plug)) => old_plug.upgrade(linked),
            None => Plugin::new(linked),
        };

        // Read config from path or string
        let config_string =
            Plugin::load_config(addon.config_path.as_ref(), addon.config_string.as_ref())?;

        // Init the addon
        let mut new_addon = new_plug.init_addon(config_string.as_deref())?;

        // Add the new plugin to the collection
        self.plugins.insert(addon.clone(), new_plug);

        // Check if we are able to upgrade from the old plugin
        let old_ver = self.addons.get(&addon.name).map(|x| x.value().version());
        if !new_addon.check_compatibility(old_ver.as_ref()) {
            self.plugins.get_mut(&addon).unwrap().rollback();
            bail!("new addon is not compatible with old version");
        }

        if let Some((_, old_addon)) = self.addons.remove(&addon.name) {
            // migrate any states/resources from old module
            new_addon.migrate(old_addon);
        };

        // Update the registries
        let engines = new_addon.engines();
        for engine in engines {
            self.engine_registry
                .insert(*engine, (PluginName::Addon(addon.name.clone()), None));
            tracing::info!("Registered addon engine {:?}", engine);
        }
        let scheduling_specs = new_addon.scheduling_specs();
        for (engine_type, spec) in scheduling_specs {
            if let Some(mut entry) = self.engine_registry.get_mut(engine_type) {
                entry.value_mut().1 = Some(*spec)
            }
        }

        // Finally insert the addon to the collection
        self.addons.insert(addon.name.clone(), new_addon);
        Ok(())
    }

    /// Load or upgrade plugins.
    /// Returns a set of affected engine types.
    pub fn load_or_upgrade_modules(
        &self,
        descriptors: &[PluginDescriptor],
    ) -> anyhow::Result<HashSet<EngineType>> {
        let mut new_modules = HashMap::with_capacity(descriptors.len());
        let mut old_verions = HashMap::with_capacity(descriptors.len());
        let mut new_versions = HashMap::with_capacity(descriptors.len() + self.modules.len());
        let modules_guard = self.modules.iter().collect::<Vec<_>>();
        for module in modules_guard.iter() {
            new_versions.insert(&module.key()[..], module.version());
        }

        // load new moduels (plugins)
        for descriptor in descriptors.iter() {
            // Get the library path and its dep file path
            let (lib_path, dep_path) = self.get_plugin_path(&descriptor);

            // RT linker load the rlib and its all transitive dependencies
            let linked = {
                let mut linker = self.rt_linker.lock().unwrap();
                linker.load_archive(lib_path, dep_path)?
            };

            // Replace the old plugin or create the new plugin
            let new_plug = match self.plugins.remove(&descriptor) {
                Some((_, old_plug)) => {
                    // upgrade from the old library
                    let old_ver = new_versions.remove(&descriptor.name[..]).unwrap();
                    old_verions.insert(&descriptor.name, old_ver);
                    old_plug.upgrade(linked)
                }
                None => {
                    // directly load the new library since it's the new
                    Plugin::new(linked)
                }
            };

            // read config from path or string
            let config_string = Plugin::load_config(
                descriptor.config_path.as_ref(),
                descriptor.config_string.as_ref(),
            )?;

            let new_module = new_plug.init_module(config_string.as_deref())?;
            self.plugins.insert(descriptor.clone(), new_plug);
            new_versions.insert(&descriptor.name, new_module.version());
            new_modules.insert(&descriptor.name, new_module);
        }

        // check compatibility
        let mut compatible = true;
        for (name, module) in new_modules.iter() {
            let old_ver = old_verions.get(*name);
            if !module.check_compatibility(old_ver, &new_versions) {
                compatible = false;
                break;
            }
        }

        if !compatible {
            // not compatible, rollback
            for desc in descriptors.iter() {
                self.plugins.get_mut(&desc).unwrap().rollback();
            }
            bail!("new modules are not compatible with existing ones");
        }

        drop(modules_guard);
        // if compatible, finish upgrade
        let mut graph_guard = self.dependency_graph.lock().unwrap();
        let mut upgraded_engine_types = HashSet::new();
        for (name, module) in new_modules.iter_mut() {
            let plugin_name = name.to_string();
            if let Some((_, old_module)) = self.modules.remove(*name) {
                // remove dependencies of the old module from the dependency graph
                let old_edges = old_module.dependencies();
                graph_guard.remove_dependency(old_edges.iter().copied())?;
                // migrate any states/resources from old module
                module.migrate(old_module);
            }
            let engines = module.engines();
            upgraded_engine_types.extend(engines.iter().copied());
            graph_guard.add_engines(engines.iter().copied());
            for engine in engines {
                self.engine_registry
                    .insert(*engine, (PluginName::Module(plugin_name.clone()), None));
            }
            let scheduling_specs = module.scheduling_specs();
            for (engine_type, spec) in scheduling_specs {
                if let Some(mut entry) = self.engine_registry.get_mut(engine_type) {
                    entry.value_mut().1 = Some(*spec)
                }
            }
        }

        for (name, module) in new_modules.into_iter() {
            let edges = module.dependencies();
            graph_guard.add_dependency(edges.iter().copied())?;
            self.modules.insert(name.to_string(), module);
        }

        for descriptor in descriptors.iter() {
            let module = self.modules.get(&descriptor.name).unwrap();
            if let Some(service_info) = module.service() {
                let dependencies = graph_guard.get_engine_dependencies(&service_info.engine)?;
                let subscription_engines = dependencies.iter().copied().collect::<HashSet<_>>();
                let mut tx_channels = service_info.tx_channels.to_vec();
                let mut rx_channels = service_info.rx_channels.to_vec();

                for channel in tx_channels.iter_mut().chain(rx_channels.iter_mut()) {
                    if !subscription_engines.contains(&channel.0)
                        || !subscription_engines.contains(&channel.1)
                    {
                        bail!(
                            "channel endpoint ({:?}, {:?}) is not in the service {:?}'s dependency graph",
                            channel.0,
                            channel.1,
                            service_info.service
                        );
                    } else {
                        // relocate &'static str
                        // COMMENT: The phoenix backend control plane uses EngineType which has a static str points
                        // to the ro memory in a dynamic library. When upgrading an engine, the static str in the
                        // old library becomes invalidate after dlclose, so before the upgrade, the backend needs
                        // to point to the new memory of the EngineType.
                        channel.0 = *subscription_engines.get(&channel.0).unwrap();
                        channel.1 = *subscription_engines.get(&channel.1).unwrap();
                    }
                }

                let groups = service_info.scheduling_groups;
                let union_find = GroupUnionFind::new(groups);

                let mut submit_groups = HashMap::new();
                let mut singleton_id = union_find.size();
                for engine in dependencies.iter().copied() {
                    let representative =
                        union_find.find_representative(engine).unwrap_or_else(|| {
                            singleton_id += 1;
                            singleton_id - 1
                        });
                    let group = submit_groups.entry(representative).or_insert_with(Vec::new);
                    group.push(engine);
                }
                let mut signatures = self.scheduling_group_signatures.lock().unwrap();
                for (_, group) in submit_groups {
                    let mut hasher = Crc32Hasher::new();
                    for engine in group.iter() {
                        Hash::hash(engine, &mut hasher);
                    }
                    let hash = hasher.finalize();
                    let group = group.into_iter().map(|x| x.0).join(" ");
                    match signatures.entry(hash) {
                        Entry::Occupied(e) => {
                            if group != *e.get() {
                                tracing::warn!(
                                    "Scheduling group signatures collided, Group 1: [{}], Group 2: [{}]",
                                    group,
                                    e.get(),
                                );
                            }
                        }
                        Entry::Vacant(e) => {
                            e.insert(group);
                        }
                    }
                }

                let service = ServiceRegistry {
                    engines: dependencies,
                    tx_channels,
                    rx_channels,
                    scheduling_groups: union_find,
                };
                tracing::info!(
                    "Registered service {:?}, dependencies={:?}",
                    service_info.service,
                    service.engines,
                );
                self.service_registry.insert(service_info.service, service);
            }
        }

        Ok(upgraded_engine_types)
    }

    /// Finish upgrade of all engines, unload old plugins
    pub(crate) fn upgrade_cleanup(&self) {
        // NOTE, we drop the old library here. To work around the issue mentioned earlier in
        // PluginManager, we do not exit thread/runtime actively.
        for mut plugin in self.plugins.iter_mut() {
            plugin.value_mut().unload_old()
        }
    }
}
