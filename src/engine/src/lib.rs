use serde::{Deserialize, Serialize};
pub use version::{version, Version};

pub(crate) mod lb;
pub mod manager;
pub mod runtime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchedulingMode {
    Dedicate,
    Spread,
    Compact,
}

pub trait Upgradable {
    fn version(&self) -> Version {
        version!().parse().unwrap()
    }

    fn check_compatible(&self, other: Version) -> bool;
    fn suspend(&mut self);
    fn dump(&self);
    fn restore(&mut self);
    fn resume(&mut self);
}

pub trait Engine: Upgradable + Send {
    fn init(&mut self);
    /// `run()` mush be non-blocking and short.
    fn run(&mut self);
    fn shutdown(&mut self);
    fn enqueue(&self);
    fn check_queue_len(&self);
}
