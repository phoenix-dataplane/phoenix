pub use version::{version, Version};

pub(crate) trait Upgradable {
    fn version(&self) -> Version {
        version!().parse().unwrap()
    }

    fn check_compatible(&self, other: Version) -> bool;
    fn suspend(&mut self);
    fn dump(&self);
    fn restore(&mut self);
}
