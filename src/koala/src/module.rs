use std::sync::Arc;
use std::{os::unix::ucred::UCred, path::Path};
use std::collections::HashMap;

use interface::engine::SchedulingMode;
use ipc::unix::DomainSocket;
use nix::unistd::Pid;
use prost_build::Module;

use crate::engine::{Engine, EngineType};
use crate::envelop::TypeTagged;
use crate::storage::{SharedStorage, ResourceCollection};

pub type ModuleCollection = HashMap<String, Arc<dyn KoalaModule>>; 

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
    fn name(&self) -> String;

    /// Engine types provided by the module
    fn engines(&self) -> &[EngineType];
    
    /// Dependencies between the engines
    /// It may include external engines
    /// Dependencies should not include other services' engines
    /// If access to other service engine's resource is needed
    /// The resources should be wrapped in ProcessShared state
    /// and managed by the corresponding module's state_mgr
    fn dependencies(&self) -> &[EngineType];

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
        shared: &mut SharedStorage,
        global: &mut ResourceCollection,
        plugged: &ModuleCollection
    ) -> Result<Box<dyn Engine>, Box<dyn std::error::Error>>;

    /// Restore and upgrade an engine from dumped states
    /// * `local`: The engine's local states
    fn restore_engine(
        &self,
        ty: &EngineType,
        local: ResourceCollection,
        glboal: &mut SharedStorage,
        global: &mut ResourceCollection,
        plugged: &ModuleCollection 
    );
}
