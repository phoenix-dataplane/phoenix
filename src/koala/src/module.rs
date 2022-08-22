use std::collections::HashMap;
use std::{os::unix::ucred::UCred, path::Path};

pub use anyhow::Result;
use dashmap::DashMap;
use interface::engine::SchedulingMode;
use ipc::unix::DomainSocket;
use nix::unistd::Pid;
pub use semver::Version;

use crate::engine::datapath::graph::ChannelDescriptor;
use crate::engine::datapath::node::DataPathNode;
use crate::engine::{Engine, EnginePair, EngineType};
use crate::envelop::TypeTagged;
use crate::storage::{ResourceCollection, SharedStorage};

pub type ModuleCollection = DashMap<String, Box<dyn KoalaModule>>;

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Service(pub &'static str);

/// Information about a service
pub struct ServiceInfo {
    /// Name of the service
    pub service: Service,
    /// Service engine that directly talks to client application
    pub engine: EngineType,
    /// Default data path tx channels between engines
    pub tx_channels: &'static [ChannelDescriptor],
    /// Default data path rx channels between engines
    pub rx_channels: &'static [ChannelDescriptor],
}

pub enum NewEngineRequest<'a> {
    Service {
        sock: &'a DomainSocket,
        client_path: &'a Path,
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
    /// The version of the module
    fn version(&self) -> Version {
        let major = env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap();
        let minor = env!("CARGO_PKG_VERSION_MINOR").parse().unwrap();
        let patch = env!("CARGO_PKG_VERSION_PATCH").parse().unwrap();
        Version::new(major, minor, patch)
    }

    /// The main service engine
    fn service(&self) -> Option<ServiceInfo>;

    /// Engine types provided by the module
    fn engines(&self) -> &[EngineType];

    /// Dependencies between the engines
    /// It may include external engines
    /// Dependencies should not include other services' engines
    /// If access to other service engine's resource is needed
    /// The resources should be wrapped in ProcessShared state
    /// and managed by the corresponding module's state_mgr
    fn dependencies(&self) -> &[EnginePair];

    /// Check whether the upgrade is compatible,
    /// provide with previous verion of current module,
    /// and all currently loaded modules' versions.
    /// For modules that are to be upgraded,
    /// `curr` contains the version after upgrade.
    fn check_compatibility(&self, prev: Option<&Version>, curr: &HashMap<&str, Version>) -> bool;

    /// Decompose (dump) the module to raw resources,
    /// e.g., dump configs, state manager into resource collection
    fn decompose(self: Box<Self>) -> ResourceCollection;

    /// Migrate states / resources from older version of the module
    /// e.g., shared state manager
    /// prev version can be obtained from version() method of `prev_module`
    /// the implementation may also call `decompose`
    /// to decompose `prev_module` into raw resources,
    /// depending on prev version
    fn migrate(&mut self, prev_module: Box<dyn KoalaModule>);

    /// Create a new engine
    /// Upon success, returns an Option
    /// Some indicates a newly created engine
    /// None indicates the action is canceled,
    /// no additional engine needed to be created.
    /// For instance, only one CmEngine in RdmaTransport
    /// should be created for each client application process
    /// * `shared`:
    ///     Message brokers and resources shared among a service
    ///     that serves the same user thread
    /// * `global`: Per user application process global resources
    /// * `plugged`:
    ///     All modules currently plugged in koala-control
    ///     Enable state sharing between engines in different services    
    fn create_engine(
        &mut self,
        ty: EngineType,
        request: NewEngineRequest,
        shared: &mut SharedStorage,
        global: &mut ResourceCollection,
        node: DataPathNode,
        plugged: &ModuleCollection,
    ) -> Result<Option<Box<dyn Engine>>>;

    /// Restore and upgrade an engine from dumped states
    /// * `local`: The engine's local states
    /// The implementation should be responsible for correctly dumping and restoring the states.
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
        &mut self,
        ty: EngineType,
        local: ResourceCollection,
        shared: &mut SharedStorage,
        global: &mut ResourceCollection,
        node: DataPathNode,
        plugged: &ModuleCollection,
        prev_version: Version,
    ) -> Result<Box<dyn Engine>>;
}

pub trait ModuleDowncast: Sized {
    fn downcast<T: KoalaModule>(self) -> Result<Box<T>, Self>;
    unsafe fn downcast_unchecked<T: KoalaModule>(self) -> Box<T>;
}

impl ModuleDowncast for Box<dyn KoalaModule> {
    #[inline]
    fn downcast<T: KoalaModule>(self) -> Result<Box<T>, Self> {
        if self.is::<T>() {
            unsafe { Ok(self.downcast_unchecked()) }
        } else {
            Err(self)
        }
    }

    #[inline]
    unsafe fn downcast_unchecked<T: KoalaModule>(self) -> Box<T> {
        debug_assert!(self.is::<T>());
        let raw: *mut dyn KoalaModule = Box::into_raw(self);
        Box::from_raw(raw as *mut T)
    }
}

impl dyn KoalaModule {
    #[inline]
    pub fn is<T: KoalaModule>(&self) -> bool {
        // Get TypeTag of the type this function is instantiated with
        let t = <T as TypeTagged>::type_tag_();

        // Get TypeTag of the type in the trait object
        let concrete = self.type_tag();

        // Compare both TypeTags on equality
        t == concrete
    }

    #[inline]
    pub fn downcast_ref<T: KoalaModule>(&self) -> Option<&T> {
        if self.is::<T>() {
            unsafe { Some(self.downcast_ref_unchecked()) }
        } else {
            None
        }
    }

    #[inline]
    pub fn downcast_mut<T: KoalaModule>(&mut self) -> Option<&mut T> {
        if self.is::<T>() {
            unsafe { Some(self.downcast_mut_unchecked()) }
        } else {
            None
        }
    }

    #[inline]
    pub unsafe fn downcast_ref_unchecked<T: KoalaModule>(&self) -> &T {
        debug_assert!(self.is::<T>());
        &*(self as *const dyn KoalaModule as *const T)
    }

    #[inline]
    pub unsafe fn downcast_mut_unchecked<T: KoalaModule>(&mut self) -> &mut T {
        debug_assert!(self.is::<T>());
        &mut *(self as *mut dyn KoalaModule as *mut T)
    }
}
