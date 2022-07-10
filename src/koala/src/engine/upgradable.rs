pub use version::{version, Version};

use super::Engine;

// dump -> unload
// restore -> reinstate
pub(crate) trait Upgradable {
    fn version(&self) -> Version {
        version!().parse().unwrap()
    }
    fn check_compatible(&self, other: Version) -> bool;
    fn suspend(&mut self);
    fn unload(self);
}
