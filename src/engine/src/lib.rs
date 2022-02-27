use interface::engine::EngineType;
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

use std::sync::mpsc::{Sender, Receiver};
pub trait RpcMessage {
    fn len(&self) -> usize;
    fn is_request(&self) -> bool;
    fn serialize(&self);
    fn deserialize(&self);
}

pub type IQueue = Receiver<Box<dyn RpcMessage>>;
pub type OQueue = Sender<Box<dyn RpcMessage>>;

pub trait Node {
    fn id(&self) -> usize;
    fn engine_type(&self) -> EngineType;
    fn tx_inputs(&self) -> Vec<IQueue>;
    fn tx_outputs(&self) -> Vec<OQueue>;
    fn rx_inputs(&self) -> Vec<IQueue>;
    fn rx_outputs(&self) -> Vec<OQueue>;
}

pub trait Engine: Upgradable + Send + Node {
    /// `resume()` mush be non-blocking and short.
    fn resume(&mut self) -> Result<EngineStatus, Box<dyn std::error::Error>>;
}

// NoProgress, MayDemandMoreCPU
pub enum EngineStatus {
    NoWork,
    Continue,
    Complete,
}
