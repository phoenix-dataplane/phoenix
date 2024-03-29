pub use anyhow::Result;
use nix::unistd::Pid;
pub use semver::Version;

use phoenix_api::engine::SchedulingMode;

use crate::engine::datapath::node::DataPathNode;
use crate::engine::{Engine, EngineType};
use crate::envelop::TypeTagged;
use crate::storage::ResourceCollection;

pub trait PhoenixAddon: TypeTagged + Send + Sync + 'static {
    /// The version of the addon
    fn version(&self) -> Version {
        let major = env!("CARGO_PKG_VERSION_MAJOR").parse().unwrap();
        let minor = env!("CARGO_PKG_VERSION_MINOR").parse().unwrap();
        let patch = env!("CARGO_PKG_VERSION_PATCH").parse().unwrap();
        Version::new(major, minor, patch)
    }

    /// Check whether it is compatible to upgrade from an older version of addon
    fn check_compatibility(&self, prev: Option<&Version>) -> bool;

    /// Decompose (dump) the module to raw resources,
    /// e.g., dump configs, state manager into resource collection
    fn decompose(self: Box<Self>) -> ResourceCollection;

    /// Migrate states / resources from older version of the addon
    fn migrate(&mut self, prev_addon: Box<dyn PhoenixAddon>);

    /// All addon engines provided in this module
    fn engines(&self) -> &[EngineType];

    /// Specify scheduling modes for some engines
    #[inline]
    fn scheduling_specs(&self) -> &[(EngineType, SchedulingMode)] {
        &[]
    }

    /// Live update addon's (RPC policy's) configuration.
    fn update_config(&mut self, config: &str) -> Result<()>;

    /// Create a new addon engine
    fn create_engine(
        &mut self,
        ty: EngineType,
        pid: Pid,
        node: DataPathNode,
    ) -> Result<Box<dyn Engine>>;

    /// Restores an addon engine
    /// addon does not share states
    /// it only dumps and restores from local states
    fn restore_engine(
        &mut self,
        ty: EngineType,
        local: ResourceCollection,
        node: DataPathNode,
        prev_version: Version,
    ) -> Result<Box<dyn Engine>>;
}

pub trait AddonDowncast: Sized {
    fn downcast<T: PhoenixAddon>(self) -> Result<Box<T>, Self>;
    unsafe fn downcast_unchecked<T: PhoenixAddon>(self) -> Box<T>;
}

impl AddonDowncast for Box<dyn PhoenixAddon> {
    #[inline]
    fn downcast<T: PhoenixAddon>(self) -> Result<Box<T>, Self> {
        if self.is::<T>() {
            unsafe { Ok(self.downcast_unchecked()) }
        } else {
            Err(self)
        }
    }

    #[inline]
    unsafe fn downcast_unchecked<T: PhoenixAddon>(self) -> Box<T> {
        debug_assert!(self.is::<T>());
        let raw: *mut dyn PhoenixAddon = Box::into_raw(self);
        Box::from_raw(raw as *mut T)
    }
}

impl dyn PhoenixAddon {
    #[inline]
    pub fn is<T: PhoenixAddon>(&self) -> bool {
        // Get TypeTag of the type this function is instantiated with
        let t = <T as TypeTagged>::type_tag_();

        // Get TypeTag of the type in the trait object
        let concrete = self.type_tag();

        // Compare both TypeTags on equality
        t == concrete
    }

    #[inline]
    pub fn downcast_ref<T: PhoenixAddon>(&self) -> Option<&T> {
        if self.is::<T>() {
            unsafe { Some(self.downcast_ref_unchecked()) }
        } else {
            None
        }
    }

    #[inline]
    pub fn downcast_mut<T: PhoenixAddon>(&mut self) -> Option<&mut T> {
        if self.is::<T>() {
            unsafe { Some(self.downcast_mut_unchecked()) }
        } else {
            None
        }
    }

    #[inline]
    pub unsafe fn downcast_ref_unchecked<T: PhoenixAddon>(&self) -> &T {
        debug_assert!(self.is::<T>());
        &*(self as *const dyn PhoenixAddon as *const T)
    }

    #[inline]
    pub unsafe fn downcast_mut_unchecked<T: PhoenixAddon>(&mut self) -> &mut T {
        debug_assert!(self.is::<T>());
        &mut *(self as *mut dyn PhoenixAddon as *mut T)
    }
}
