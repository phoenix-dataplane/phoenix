use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::mem::ManuallyDrop;
use std::path::Path;
use std::sync::Mutex;

use anyhow::bail;
use crc32fast::Hasher as Crc32Hasher;
use dashmap::DashMap;
use interface::engine::SchedulingMode;
use itertools::Itertools;

use ipc::control::PluginDescriptor;

use crate::addon::KoalaAddon;
use crate::dependency::EngineGraph;
use crate::engine::datapath::graph::ChannelDescriptor;
use crate::engine::group::GroupUnionFind;
use crate::engine::EngineType;
use crate::module::KoalaModule;
use crate::module::Service;

// Re-export for plugin dynamic library's use
pub type InitFnResult<T> = anyhow::Result<T>;

pub type InitModuleFn = fn(Option<&str>) -> InitFnResult<Box<dyn KoalaModule>>;
pub type InitAddonFn = fn(Option<&str>) -> InitFnResult<Box<dyn KoalaAddon>>;

pub struct DynamicLibrary {
    lib: libloading::Library,
    _old: Option<libloading::Library>,
}

impl DynamicLibrary {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let lib = unsafe { libloading::Library::new(path.as_ref()).unwrap() };
        DynamicLibrary { lib, _old: None }
    }

    pub fn init_module(&self, config_string: Option<&str>) -> InitFnResult<Box<dyn KoalaModule>> {
        let func = unsafe { self.lib.get::<InitModuleFn>(b"init_module").unwrap() };
        func(config_string)
    }

    pub fn init_addon(&self, config_string: Option<&str>) -> InitFnResult<Box<dyn KoalaAddon>> {
        let func = unsafe { self.lib.get::<InitAddonFn>(b"init_addon").unwrap() };
        func(config_string)
    }

    pub fn upgrade<P: AsRef<Path>>(self, path: P) -> Self {
        let new = unsafe { libloading::Library::new(path.as_ref()).unwrap() };
        let old = self.lib;
        DynamicLibrary {
            lib: new,
            _old: Some(old),
        }
    }

    #[inline]
    pub fn unload_old(&mut self) {
        self._old = None
    }

    #[inline]
    pub fn rollback(&mut self) {
        if self._old.is_some() {
            self.lib = self._old.take().unwrap();
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Plugin {
    /// Regular koala plugin (service)
    Module(String),
    /// Koala addons
    Addon(String),
}

pub(crate) struct ServiceRegistry {
    pub(crate) engines: Vec<EngineType>,
    pub(crate) tx_channels: Vec<ChannelDescriptor>,
    pub(crate) rx_channels: Vec<ChannelDescriptor>,
    pub(crate) scheduling_groups: GroupUnionFind,
}

// COMMENT(wyj): drop order matters
pub struct PluginCollection {
    pub(crate) modules: DashMap<String, Box<dyn KoalaModule>>,
    pub(crate) addons: DashMap<String, Box<dyn KoalaAddon>>,
    pub(crate) engine_registry: DashMap<EngineType, (Plugin, Option<SchedulingMode>)>,
    pub(crate) service_registry: DashMap<Service, ServiceRegistry>,
    dependency_graph: Mutex<EngineGraph>,
    /// signature of different scheduling groups
    /// that service engines (addon engines are excluded) will create
    scheduling_group_signatures: Mutex<HashMap<u32, String>>,

    // There seems to be a fundmental problem of dlclose with thread-local destructors.
    //
    // The observed behavior is that when libstd is dynamically linked, a dynamic library who had
    // referred thread-local variables will register thread-local dtors which will be executed
    // at the the time of thread being exited. If the dynamic library was unloaded before via
    // dlclose, those dtors will point to potentially freed memory and cause immediate UB or
    // later segmentation faults.
    //
    // In short, such case happens when (1). linking libstd dynamically using prefer-dyanmic; (2).
    // dlopen some libmycrate.so and initializing a thread-local variable; (3) unload the library
    // libmycrate.so with dlclose; (4) thread of that tls is exited; (5) segmentation fault
    //
    // There have been many threads discussing this issue and the potential solutions.
    //
    // https://github.com/rust-lang/rust/issues/88737
    // https://github.com/nagisa/rust_libloading/issues/41
    // https://github.com/rust-lang/rust/issues/28794
    // https://github.com/nagisa/rust_libloading/issues/5
    //
    // In short, to work around this issue, we only need to break any condition mentioned above.
    // macOS makes call_tls_dtor a no-op. Some other common workaround are not to drop the library.
    // In our situation, our current workaround is not dropped the library here (`ManuallyDrop`).
    // In case `PluginCollection::upgrade_cleanup` still drops the old libraries, our current
    // workaround is not to stop the runtime, so hopefully no thread exits.
    libraries: ManuallyDrop<DashMap<Plugin, DynamicLibrary>>,
}

// impl Drop for PluginCollection {
//     fn drop(&mut self) {
//         dbg!(self.libraries.len());
//
//         // Salloc is the one who cause the issue. Even though we don't find it refer to any
//         // thread-locals.
//
//         // let rpc_adapter = self.libraries.remove(&Plugin::Module("RpcAdapter".to_string())).unwrap();
//         // std::mem::forget(rpc_adapter);
//         // let rdma_transport = self.libraries.remove(&Plugin::Module("RdmaTransport".to_string())).unwrap();
//         // std::mem::forget(rdma_transport);
//         let a1 = self.libraries.remove(&Plugin::Module("Salloc".to_string())).unwrap();
//         std::mem::forget(a1);
//         // let a2 = self.libraries.remove(&Plugin::Module("Mrpc".to_string())).unwrap();
//         // std::mem::forget(a2);
//         // let a3 = self.libraries.remove(&Plugin::Addon("RateLimit".to_string())).unwrap();
//         // std::mem::forget(a3);
//     }
// }

impl PluginCollection {
    /// Returns an empty PluginCollection.
    pub fn new() -> Self {
        PluginCollection {
            modules: DashMap::new(),
            addons: DashMap::new(),
            engine_registry: DashMap::new(),
            service_registry: DashMap::new(),
            dependency_graph: Mutex::new(EngineGraph::new()),
            scheduling_group_signatures: Mutex::new(HashMap::new()),
            libraries: ManuallyDrop::new(DashMap::new()),
        }
    }

    /// Load config string from either inline multiline string or a separate path.
    pub(crate) fn load_config<P: AsRef<Path>, S: AsRef<str>>(
        config_path: Option<P>,
        config_string: Option<S>,
    ) -> anyhow::Result<Option<String>> {
        if config_path.is_some() && config_string.is_some() {
            bail!("Please provide either `config_path` or `config_string`, not both");
        }

        if let Some(path) = config_path {
            Ok(Some(std::fs::read_to_string(path)?))
        } else {
            Ok(config_string.map(|x| x.as_ref().to_string()))
        }
    }

    pub fn load_or_upgrade_addon(&self, addon: &PluginDescriptor) -> anyhow::Result<()> {
        let old_ver = self.addons.get(&addon.name).map(|x| x.value().version());
        let plugin = Plugin::Addon(addon.name.clone());
        let old_dylib = self.libraries.remove(&plugin);
        let dylib = if let Some((_, old)) = old_dylib {
            old.upgrade(&addon.lib_path)
        } else {
            DynamicLibrary::new(&addon.lib_path)
        };

        // read config from path or string
        let config_string =
            Self::load_config(addon.config_path.as_ref(), addon.config_string.as_ref())?;
        let mut new_addon = dylib.init_addon(config_string.as_deref())?;

        self.libraries.insert(plugin.clone(), dylib);
        if !new_addon.check_compatibility(old_ver.as_ref()) {
            self.libraries.get_mut(&plugin).unwrap().rollback();
            bail!("new addon is not compatible with old version");
        }

        if let Some((_, old_addon)) = self.addons.remove(&addon.name) {
            // migrate any states/resources from old module
            new_addon.migrate(old_addon);
        };
        let engines = new_addon.engines();
        for engine in engines {
            self.engine_registry.insert(*engine, (plugin.clone(), None));
            tracing::info!("Registered addon engine {:?}", engine);
        }
        let scheduling_specs = new_addon.scheduling_specs();
        for (engine_type, spec) in scheduling_specs {
            if let Some(mut entry) = self.engine_registry.get_mut(engine_type) {
                entry.value_mut().1 = Some(*spec)
            }
        }

        self.addons.insert(addon.name.clone(), new_addon);
        Ok(())
    }

    /// Load or upgrade plugins.
    /// Returns a set of affected engine types.
    pub fn load_or_upgrade_modules(
        &self,
        descriptors: &Vec<PluginDescriptor>,
    ) -> anyhow::Result<HashSet<EngineType>> {
        let plugins = descriptors
            .iter()
            .map(|x| Plugin::Module(x.name.clone()))
            .collect::<Vec<_>>();

        let mut new_modules = HashMap::with_capacity(descriptors.len());
        let mut old_verions = HashMap::with_capacity(descriptors.len());
        let mut new_versions = HashMap::with_capacity(descriptors.len() + self.modules.len());
        let modules_guard = self.modules.iter().collect::<Vec<_>>();
        for module in modules_guard.iter() {
            new_versions.insert(&module.key()[..], module.version());
        }

        // load new moduels (plugins)
        for (descriptor, plugin) in descriptors.iter().zip(plugins.iter()) {
            let old_dylib = self.libraries.remove(plugin);

            let dylib = if let Some((_, old)) = old_dylib {
                // upgrade from the old library
                let old_ver = new_versions.remove(&descriptor.name[..]).unwrap();
                old_verions.insert(&descriptor.name[..], old_ver);
                old.upgrade(&descriptor.lib_path)
            } else {
                // directly load the new library since it's the new
                DynamicLibrary::new(&descriptor.lib_path)
            };

            // read config from path or string
            let config_string = Self::load_config(
                descriptor.config_path.as_ref(),
                descriptor.config_string.as_ref(),
            )?;
            let new_module = dylib.init_module(config_string.as_deref())?;
            self.libraries.insert(plugin.clone(), dylib);
            new_versions.insert(&descriptor.name[..], new_module.version());
            new_modules.insert(&descriptor.name[..], new_module);
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
            for plugin in plugins.iter() {
                self.libraries.get_mut(plugin).unwrap().rollback();
            }
            bail!("new modules are not compatible with existing ones");
        }

        drop(modules_guard);
        // if compatible, finish upgrade
        let mut graph_guard = self.dependency_graph.lock().unwrap();
        let mut upgraded_engine_types = HashSet::new();
        for (name, module) in new_modules.iter_mut() {
            let plugin = Plugin::Module(name.to_string());
            if let Some((_, old_module)) = self.modules.remove(*name) {
                // migrate any states/resources from old module
                module.migrate(old_module);
            }
            let engines = module.engines();
            upgraded_engine_types.extend(engines.iter().copied());
            graph_guard.add_engines(engines.iter().copied());
            for engine in engines {
                self.engine_registry.insert(*engine, (plugin.clone(), None));
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
                        // COMMENT: The koala backend control plane uses EngineType which has a static str points
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
                            if &*group != &**e.get() {
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
        // PluginCollection, we do not exit thread/runtime actively.
        for mut plugin in self.libraries.iter_mut() {
            plugin.value_mut().unload_old()
        }
    }
}
