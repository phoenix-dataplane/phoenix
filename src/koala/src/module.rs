use std::{os::unix::ucred::UCred, path::Path};

use dashmap::DashMap;
use interface::engine::SchedulingMode;
use ipc::unix::DomainSocket;
use nix::unistd::Pid;

use crate::engine::{Engine, EnginePair, EngineType};
use crate::envelop::TypeTagged;
use crate::storage::{ResourceCollection, SharedStorage};

pub type ModuleCollection = DashMap<String, Box<dyn KoalaModule>>;

#[repr(transparent)]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Service(pub String);

pub enum NewEngineRequest<'a> {
    Service {
        sock: &'a DomainSocket,
        cleint_path: &'a Path,
        mode: SchedulingMode,
        cred: &'a UCred,
    },
    Auxiliary {
        pid: Pid,
        mode: SchedulingMode,
    },
}

// TODO(wyj): figure out whether we want to share modules between threads
// i.e., restoring engines in dedicated thread or on main control thread?
pub trait KoalaModule: TypeTagged + Send + Sync + 'static {
    /// The module's name
    fn name(&self) -> &str;

    /// The main service engine
    fn service(&self) -> (Service, EngineType);

    /// Engine types provided by the module
    fn engines(&self) -> Vec<EngineType>;

    /// Dependencies between the engines
    /// It may include external engines
    /// Dependencies should not include other services' engines
    /// If access to other service engine's resource is needed
    /// The resources should be wrapped in ProcessShared state
    /// and managed by the corresponding module's state_mgr
    fn dependencies(&self) -> Vec<EnginePair>;

    /// Create a new engine
    /// * `shared`:
    ///     Message brokers and resources shared among a service
    ///     that serves the same user thread
    /// * `global`: Per user application process global resources
    /// * `plugged`:
    ///     All modules currently plugged in koala-control
    ///     Enable state sharing between engines in different services    
    fn create_engine(
        &self,
        ty: &EngineType,
        request: NewEngineRequest,
        // shared: &mut SharedStorage,
        // global: &mut ResourceCollection,
        // plugged: &ModuleCollection
    ) -> Result<Box<dyn Engine>, Box<dyn std::error::Error>>;

    /// Restore and upgrade an engine from dumped states
    /// * `local`: The engine's local states
    /// The implementation should be responsible for correctly dumping and restoring the states/
    /// Depending on previous engines' version, different actions may be taken.
    /// For instance, if engine B uses engine A's shared state,
    /// then engine A and B must be jointly upgrade,
    /// if the share state's type needs to be upgraded.
    /// In this case, the last engine to shutdown will unwrap the Arc
    /// and extract the wrapped shared state, dump it in into global resource.
    /// If states are not upgraded, engine A can upgrade alone.
    /// In this case, the new module must migrate the state_mgr from previous module,
    /// as the Arc will still be hold by some engines.
    /// The implementation must correctly interpret previous engine's dumped states
    /// In case some of the states' types are changed in an upgrade,
    /// engines should dumped their states to atomic components,
    /// so that the new state type can be reassembled from the components.
    fn restore_engine(
        &self,
        ty: &EngineType,
        local: ResourceCollection,
        // shared: &mut SharedStorage,
        // global: &mut ResourceCollection,
        // plugged: &ModuleCollection
    ) -> Result<Box<dyn Engine>, Box<dyn std::error::Error>>;
}

pub struct ModulePlugin {}
