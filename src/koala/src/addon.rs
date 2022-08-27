pub use anyhow::Result;
use nix::unistd::Pid;
pub use semver::Version;

use crate::engine::datapath::node::DataPathNode;
use crate::engine::{Engine, EngineType};
use crate::envelop::TypeTagged;
use crate::storage::ResourceCollection;

pub trait KoalaAddon: TypeTagged + Send + Sync + 'static {
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
    fn migrate(&mut self, prev_addon: Box<dyn KoalaAddon>);

    /// All addon engines provided in this module
    fn engines(&self) -> &[EngineType];

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
    fn downcast<T: KoalaAddon>(self) -> Result<Box<T>, Self>;
    unsafe fn downcast_unchecked<T: KoalaAddon>(self) -> Box<T>;
}

impl AddonDowncast for Box<dyn KoalaAddon> {
    #[inline]
    fn downcast<T: KoalaAddon>(self) -> Result<Box<T>, Self> {
        if self.is::<T>() {
            unsafe { Ok(self.downcast_unchecked()) }
        } else {
            Err(self)
        }
    }

    #[inline]
    unsafe fn downcast_unchecked<T: KoalaAddon>(self) -> Box<T> {
        debug_assert!(self.is::<T>());
        let raw: *mut dyn KoalaAddon = Box::into_raw(self);
        Box::from_raw(raw as *mut T)
    }
}

impl dyn KoalaAddon {
    #[inline]
    pub fn is<T: KoalaAddon>(&self) -> bool {
        // Get TypeTag of the type this function is instantiated with
        let t = <T as TypeTagged>::type_tag_();

        // Get TypeTag of the type in the trait object
        let concrete = self.type_tag();

        // Compare both TypeTags on equality
        t == concrete
    }

    #[inline]
    pub fn downcast_ref<T: KoalaAddon>(&self) -> Option<&T> {
        if self.is::<T>() {
            unsafe { Some(self.downcast_ref_unchecked()) }
        } else {
            None
        }
    }

    #[inline]
    pub fn downcast_mut<T: KoalaAddon>(&mut self) -> Option<&mut T> {
        if self.is::<T>() {
            unsafe { Some(self.downcast_mut_unchecked()) }
        } else {
            None
        }
    }

    #[inline]
    pub unsafe fn downcast_ref_unchecked<T: KoalaAddon>(&self) -> &T {
        debug_assert!(self.is::<T>());
        &*(self as *const dyn KoalaAddon as *const T)
    }

    #[inline]
    pub unsafe fn downcast_mut_unchecked<T: KoalaAddon>(&mut self) -> &mut T {
        debug_assert!(self.is::<T>());
        &mut *(self as *mut dyn KoalaAddon as *mut T)
    }
}
