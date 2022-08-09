use nix::unistd::Pid;
pub use anyhow::Result;
pub use semver::Version;

use crate::envelop::TypeTagged;
use crate::engine::{Engine, EngineType};
use crate::engine::datapath::node::DataPathNode;
use crate::storage::ResourceCollection;


pub trait Addon: TypeTagged + Send + Sync + 'static {
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
    fn migrate(&mut self, prev_addon: Box<dyn Addon>);

    /// All addon engines provided in this module
    fn engines(&self) -> Vec<EngineType>;

    /// Create a new addon engine
    fn create_engine(
        &mut self,
        ty: &EngineType,
        pid: Pid,
        node: DataPathNode,
    ) -> Result<Box<dyn Engine>>;

    /// Restores an addon engine
    /// addon does not share states
    /// it only dumps and restores from local states
    fn restore_engine(
        &mut self,
        local: ResourceCollection,
        node: DataPathNode,
        prev_version: Version,
    ) -> Result<Box<dyn Engine>>;
}