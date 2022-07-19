use std::path::Path;
use std::sync::Mutex;

use dashmap::DashMap;

use crate::dependency::EngineGraph;
use crate::engine::EngineType;
use crate::module::KoalaModule;
use crate::module::Service;

pub struct Plugin {
    lib: libloading::Library,
    _old: Option<libloading::Library>,
}

impl Plugin {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let lib = unsafe { libloading::Library::new(path.as_ref()).unwrap() };
        Plugin { lib, _old: None }
    }

    pub fn init_module(&self) -> Box<dyn KoalaModule> {
        let func = unsafe {
            self.lib
                .get::<unsafe fn() -> Box<dyn KoalaModule>>(b"init_module")
                .unwrap()
        };
        let module = unsafe { func() };
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

    pub fn unload_old(&mut self) {
        self._old = None
    }
}

pub struct PluginCollection {
    plugins: DashMap<String, Plugin>,
    pub modules: DashMap<String, Box<dyn KoalaModule>>,
    pub engine_registry: DashMap<EngineType, String>,
    pub service_registry: DashMap<Service, Vec<EngineType>>,
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

    pub fn load_plugin<P: AsRef<Path>>(&self, name: String, path: P) {
        let old = self.plugins.remove(&name);
        if let Some((_, old)) = old {
            self.plugins.insert(name.clone(), old.upgrade(path));
        } else {
            self.plugins.insert(name.clone(), Plugin::new(path));
        }

        let plugin = self.plugins.get_mut(&name).unwrap();
        let module = plugin.init_module();
        let engines = module.engines();
        for engine in engines {
            self.engine_registry.insert(engine.clone(), name.clone());
        }

        let edges = module.dependencies();
        self.graph.lock().unwrap().add_dependency(&edges[..]);

        let (service_name, service_engine) = module.service();
        self.add_service(service_name.clone(), service_engine.clone());

        self.modules.insert(name, module);
    }

    fn add_service(&self, service: Service, engine: EngineType) {
        let dependencies = self.graph.lock().unwrap().get_engine_dependencies(&engine);
        self.service_registry.insert(service, dependencies);
    }
}
