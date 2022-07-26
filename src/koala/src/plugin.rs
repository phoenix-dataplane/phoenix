use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use anyhow;
use anyhow::bail;

use dashmap::DashMap;
use ipc::control::PluginDescriptor;

use crate::dependency::EngineGraph;
use crate::engine::EngineType;
use crate::module::KoalaModule;
use crate::module::Service;

pub type InitFn = fn(Option<&Path>) -> Box<dyn KoalaModule>;

pub struct Plugin {
    lib: libloading::Library,
    _old: Option<libloading::Library>,
}

impl Plugin {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let lib = unsafe { libloading::Library::new(path.as_ref()).unwrap() };
        Plugin { lib, _old: None }
    }

    pub fn init_module(&self, config_path: Option<&Path>) -> Box<dyn KoalaModule> {
        let func = unsafe { self.lib.get::<InitFn>(b"init_module").unwrap() };
        let module = func(config_path);
        module
    }

    pub fn upgrade<P: AsRef<Path>>(self, path: P) -> Self {
        let new = unsafe { libloading::Library::new(path.as_ref()).unwrap() };
        let old = self.lib;
        Plugin {
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

pub struct PluginCollection {
    plugins: DashMap<String, Plugin>,
    pub(crate) modules: DashMap<String, Box<dyn KoalaModule>>,
    pub(crate) engine_registry: DashMap<EngineType, String>,
    pub(crate) service_registry: DashMap<Service, Vec<EngineType>>,
    graph: Mutex<EngineGraph>,
}

impl PluginCollection {
    pub fn new() -> Self {
        PluginCollection {
            plugins: DashMap::new(),
            modules: DashMap::new(),
            engine_registry: DashMap::new(),
            service_registry: DashMap::new(),
            graph: Mutex::new(EngineGraph::new()),
        }
    }

    /// Load or upgrade plugins
    /// Returns a set of affected engine types
    pub fn load_or_upgrade_plugins(
        &self,
        descriptors: &Vec<PluginDescriptor>,
    ) -> anyhow::Result<HashSet<EngineType>> {
        let mut new_modules = HashMap::new();
        let mut old_verions = HashMap::new();
        let mut new_versions = HashMap::new();
        let modules_guard = self.modules.iter().collect::<Vec<_>>();
        for module in modules_guard.iter() {
            new_versions.insert(&module.key()[..], module.value().version());
        }

        // load new moduels (plugins)
        for descriptor in descriptors.iter() {
            let old = self.plugins.remove(&descriptor.name);
            if let Some((_, old)) = old {
                let old_ver = new_versions.get(&descriptor.name[..]).unwrap().clone();
                old_verions.insert(&descriptor.name[..], old_ver);
                self.plugins
                    .insert(descriptor.name.clone(), old.upgrade(&descriptor.lib_path));
            } else {
                self.plugins
                    .insert(descriptor.name.clone(), Plugin::new(&descriptor.lib_path));
            }
            let new_plugin = self.plugins.get_mut(&descriptor.name).unwrap();
            let config_path = descriptor.config_path.as_ref().map(|x| x.as_path());
            let new_module = new_plugin.init_module(config_path);
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
            for descriptor in descriptors.iter() {
                self.plugins.get_mut(&descriptor.name).unwrap().rollback();
            }
            bail!("New plugins are not compatible with existing ones");
        }

        std::mem::drop(modules_guard);
        // if compatible, finish upgrade
        let mut graph_guard = self.graph.lock().unwrap();
        let mut upgraded_engine_types = HashSet::new();
        for (name, mut module) in new_modules.into_iter() {
            if let Some((_, old_module)) = self.modules.remove(name) {
                // migrate any states/resources from old module
                module.migrate(old_module);
            }
            let engines = module.engines();
            upgraded_engine_types.extend(engines.iter().cloned());
            graph_guard.add_engines(&engines[..]);
            for engine in engines {
                self.engine_registry
                    .insert(engine.clone(), name.to_string());
            }

            let edges = module.dependencies();
            graph_guard.add_dependency(&edges[..]);

            self.modules.insert(name.to_string(), module);
        }

        for descriptor in descriptors.iter() {
            let module = self.modules.get(&descriptor.name).unwrap();
            let (service_name, service_engine) = module.service();
            let dependencies = graph_guard.get_engine_dependencies(&service_engine);
            self.service_registry.insert(service_name, dependencies);
        }
        Ok(upgraded_engine_types)
    }

    /// Finish upgrade of all engines, unload old plugins
    pub(crate) fn upgrade_cleanup(&self) {
        for mut plugin in self.plugins.iter_mut() {
            plugin.value_mut().unload_old()
        }
    }

    // fn add_service(&self, service: Service, engine: EngineType) {
    //     let dependencies = self.graph.lock().unwrap().get_engine_dependencies(&engine);
    //     self.service_registry.insert(service, dependencies);
    // }
}
