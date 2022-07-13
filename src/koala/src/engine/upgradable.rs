pub use version::{version, Version};

// dump -> unload
// restore -> reinstate
pub trait Upgradable {
    fn version(&self) -> Version {
        version!().parse().unwrap()
    }
    fn check_compatible(&self, other: Version) -> bool;
    fn suspend(&mut self);
    fn unload(self);
}
