use super::channel::{Receiver, Sender};
use super::message::{EngineRxMessage, EngineTxMessage};
use crate::engine::EngineType;

pub type TxIQueue = Receiver<EngineTxMessage>;
pub type TxOQueue = Sender<EngineTxMessage>;

pub type RxIQueue = Receiver<EngineRxMessage>;
pub type RxOQueue = Sender<EngineRxMessage>;

pub trait Vertex {
    fn tx_inputs(&mut self) -> &mut Vec<TxIQueue>;
    fn tx_outputs(&mut self) -> &mut Vec<TxOQueue>;
    fn rx_inputs(&mut self) -> &mut Vec<RxIQueue>;
    fn rx_outputs(&mut self) -> &mut Vec<RxOQueue>;
}

/// A descriptor to describe channel
#[derive(Debug, Clone, Copy)]
pub struct ChannelDescriptor(pub EngineType, pub EngineType, pub usize, pub usize);

pub struct DataPathNode {
    pub tx_inputs: Vec<TxIQueue>,
    pub tx_outputs: Vec<TxOQueue>,
    pub rx_inputs: Vec<RxIQueue>,
    pub rx_outputs: Vec<RxOQueue>,
}

impl DataPathNode {
    pub fn new() -> Self {
        DataPathNode {
            tx_inputs: Vec::new(),
            tx_outputs: Vec::new(),
            rx_inputs: Vec::new(),
            rx_outputs: Vec::new(),
        }
    }
}

#[macro_export]
macro_rules! impl_vertex_for_engine {
    ($engine:ident, $node:ident $($vars:tt)*) => {
        impl$($vars)* phoenix_common::engine::datapath::node::Vertex for $engine$($vars)* {
            #[inline]
            fn tx_inputs(&mut self) -> &mut Vec<phoenix_common::engine::datapath::node::TxIQueue> {
                &mut self.$node.tx_inputs
            }
            fn tx_outputs(&mut self) -> &mut Vec<phoenix_common::engine::datapath::node::TxOQueue> {
                &mut self.$node.tx_outputs
            }
            fn rx_inputs(&mut self) -> &mut Vec<phoenix_common::engine::datapath::node::RxIQueue> {
                &mut self.$node.rx_inputs
            }
            fn rx_outputs(&mut self) -> &mut Vec<phoenix_common::engine::datapath::node::RxOQueue> {
                &mut self.$node.rx_outputs
            }
        }
    };
}
