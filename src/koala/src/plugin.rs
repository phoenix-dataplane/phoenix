use std::collections::{HashSet, HashMap};
use std::path::Path;
use std::sync::Mutex;

use anyhow;
use anyhow::bail;

use dashmap::DashMap;
use ipc::control::PluginDescriptor;

use crate::addon::Addon;
use crate::dependency::EngineGraph;
use crate::engine::EngineType;
use crate::engine::datapath::graph::ChannelDescriptor;
use crate::module::KoalaModule;
use crate::module::Service;

pub type InitModuleFn = fn(Option<&Path>) -> Box<dyn KoalaModule>;
pub type InitAddonFn = fn(Option<&Path>) -> Box<dyn Addon>;

pub struct DynamicLibrary {
    lib: libloading::Library,
    _old: Option<libloading::Library>,
}

impl DynamicLibrary {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let lib = unsafe { libloading::Library::new(path.as_ref()).unwrap() };
        DynamicLibrary { lib, _old: None }
    }

    pub fn init_module(&self, config_path: Option<&Path>) -> Box<dyn KoalaModule> {
        let func = unsafe { self.lib.get::<InitModuleFn>(b"init_module").unwrap() };
        let module = func(config_path);
        module
    }

    pub fn init_addon(&self, config_path: Option<&Path>) -> Box<dyn Addon> {
        let func = unsafe { self.lib.get::<InitAddonFn>(b"init_addon").unwrap() };
        let addon = func(config_path);
        addon
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

pub struct ServiceRegistry {
    pub(crate) engines: Vec<EngineType>,
    pub(crate) tx_channels: Vec<ChannelDescriptor>,
    pub(crate) rx_channels: Vec<ChannelDescriptor>,
}

pub struct PluginCollection {
    libraries: DashMap<Plugin, DynamicLibrary>,
    pub(crate) modules: DashMap<String, Box<dyn KoalaModule>>,
    pub(crate) addons: DashMap<String, Box<dyn Addon>>,
    pub(crate) engine_registry: DashMap<EngineType, Plugin>,
    pub(crate) service_registry: DashMap<Service, ServiceRegistry>,
    dependency_graph: Mutex<EngineGraph>,
}

impl PluginCollection {
    pub fn new() -> Self {
        PluginCollection {
            libraries: DashMap::new(),
            modules: DashMap::new(),
            addons: DashMap::new(),
            engine_registry: DashMap::new(),
            service_registry: DashMap::new(),
            dependency_graph: Mutex::new(EngineGraph::new()),
        }
    }

    pub fn load_or_upgrade_addon(
        &self,
        addon: PluginDescriptor
    ) {

        let plugin = Plugin::Addon(addon.name);
        let old_dylib = self.libraries.remove(&plugin);
        if let Some((_, old)) = old_dylib {

        }


    }

    /// Load or upgrade plugins
    /// Returns a set of affected engine types
    pub fn load_or_upgrade_plugins(
        &self,
        descriptors: &Vec<PluginDescriptor>,
    ) -> anyhow::Result<HashSet<EngineType>> {
        let plugins = descriptors.iter()
            .map(|x| Plugin::Module(x.name.clone()))
            .collect::<Vec<_>>();

        let mut new_modules = HashMap::new();
        let mut old_verions = HashMap::new();
        let mut new_versions = HashMap::new();
        let modules_guard = self.modules.iter().collect::<Vec<_>>();
        for module in modules_guard.iter() {
            new_versions.insert(&module.key()[..], module.value().version());
        }

        // load new moduels (plugins)
        for (descriptor, plugin) in descriptors.iter().zip(plugins.iter()) {
            let old_dylib = self.libraries.remove(plugin);
            let dylib = if let Some((_, old)) = old_dylib {
                let old_ver = new_versions.get(&descriptor.name[..]).unwrap().clone();
                old_verions.insert(&descriptor.name[..], old_ver);
                old.upgrade(&descriptor.lib_path)
            } else {
                DynamicLibrary::new(&descriptor.lib_path)
            };

            let config_path = descriptor.config_path.as_ref().map(|x| x.as_path());
            let new_module = dylib.init_module(config_path);
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
            bail!("New plugins are not compatible with existing ones");
        }

        std::mem::drop(modules_guard);
        // if compatible, finish upgrade
        let mut graph_guard = self.dependency_graph.lock().unwrap();
        let mut upgraded_engine_types = HashSet::new();
        for (name, mut module) in new_modules.into_iter() {
            let plugin = Plugin::Module(name.to_string());
            if let Some((_, old_module)) = self.modules.remove(name) {
                // migrate any states/resources from old module
                module.migrate(old_module);
            }
            let engines = module.engines();
            upgraded_engine_types.extend(engines.iter().cloned());
            graph_guard.add_engines(&engines[..]);
            for engine in engines {
                self.engine_registry
                    .insert(engine.clone(), plugin.clone());
            }

            let edges = module.dependencies();
            graph_guard.add_dependency(&edges[..]);

            self.modules.insert(name.to_string(), module);
        }

        for descriptor in descriptors.iter() {
            let module = self.modules.get(&descriptor.name).unwrap();
            let (service_name, service_engine) = module.service();
            let dependencies = graph_guard.get_engine_dependencies(&service_engine);
            eprintln!("Service={:?}, Dependencies {:?}", service_name, dependencies);
            let group_engines = dependencies.iter().cloned().collect::<HashSet<_>>();
            let tx_channels = module.tx_channels();
            let rx_channels = module.rx_channels(); 
            for channel in tx_channels.iter().chain(rx_channels.iter()) {
                if !group_engines.contains(&channel.0) || !group_engines.contains(&channel.1) {
                    bail!("Channel endpoint ({:?}, {:?}) is not in the service {:?}'s dependency graph", channel.0, channel.1, service_name);
                }
            }

            let service = ServiceRegistry {
                engines: dependencies,
                tx_channels,
                rx_channels,
            };
            self.service_registry.insert(service_name, service);
        }

        Ok(upgraded_engine_types)
    }

    /// Finish upgrade of all engines, unload old plugins
    pub(crate) fn upgrade_cleanup(&self) {
        for mut plugin in self.libraries.iter_mut() {
            plugin.value_mut().unload_old()
        }
    }
}
