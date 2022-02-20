pub use version::{version, Version};

pub(crate) mod lb;
pub mod manager;
pub mod runtime;

pub trait Upgradable {
    fn version(&self) -> Version {
        version!().parse().unwrap()
    }

    fn check_compatible(&self, other: Version) -> bool;
    fn suspend(&mut self);
    fn dump(&self);
    fn restore(&mut self);
}

pub trait Engine: Upgradable + Send {
    /// `resume()` mush be non-blocking and short.
    fn resume(&mut self) -> Result<EngineStatus, Box<dyn std::error::Error>>;
}

// NoProgress, MayDemandMoreCPU
pub enum EngineStatus {
    NoWork,
    Continue,
    Complete,
}
