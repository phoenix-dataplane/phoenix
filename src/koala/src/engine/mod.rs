use interface::engine::EngineType;
pub use version::{version, Version};

pub mod manager;
pub(crate) mod lb;
pub(crate) mod runtime;

pub(crate) trait Upgradable {
    fn version(&self) -> Version {
        version!().parse().unwrap()
    }

    fn check_compatible(&self, other: Version) -> bool;
    fn suspend(&mut self);
    fn dump(&self);
    fn restore(&mut self);
}

use std::sync::mpsc::{Sender, Receiver};
use crate::mrpc::marshal::RpcMessage;

pub(crate) type IQueue = Receiver<Box<dyn RpcMessage>>;
pub(crate) type OQueue = Sender<Box<dyn RpcMessage>>;

pub(crate) trait Vertex {
    fn id(&self) -> &str;
    fn engine_type(&self) -> EngineType;
    fn tx_inputs(&self) -> &Vec<IQueue>;
    fn tx_outputs(&self) -> &Vec<OQueue>;
    fn rx_inputs(&self) -> &Vec<IQueue>;
    fn rx_outputs(&self) -> &Vec<OQueue>;
}

pub(crate) trait Engine: Upgradable + Send + Vertex {
    /// `resume()` mush be non-blocking and short.
    fn resume(&mut self) -> Result<EngineStatus, Box<dyn std::error::Error>>;
    #[inline]
    unsafe fn tls(&self) -> Option<&'static dyn std::any::Any> {
        None
    }
}

// NoProgress, MayDemandMoreCPU
pub(crate) enum EngineStatus {
    NoWork,
    Continue,
    Complete,
}

// pub struct EngineTlStorage {
//     pub ptr: Box<dyn std::any::Any>,
// }
