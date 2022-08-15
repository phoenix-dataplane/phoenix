use std::collections::HashMap;

use crossbeam::channel::{Receiver, Sender};

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

#[macro_export]
macro_rules! impl_vertex_for_engine {
    ($engine:ident, $node:ident) => {
        impl koala::engine::datapath::graph::Vertex for $engine {
            #[inline]
            fn tx_inputs(&mut self) -> &mut Vec<koala::engine::datapath::graph::TxIQueue> {
                &mut self.$node.tx_inputs
            }
            fn tx_outputs(&mut self) -> &mut Vec<koala::engine::datapath::graph::TxOQueue> {
                &mut self.$node.tx_outputs
            }
            fn rx_inputs(&mut self) -> &mut Vec<koala::engine::datapath::graph::RxIQueue> {
                &mut self.$node.rx_inputs
            }
            fn rx_outputs(&mut self) -> &mut Vec<koala::engine::datapath::graph::RxOQueue> {
                &mut self.$node.rx_outputs
            }
        }
    };
}

/// A descriptor to describe channel
#[derive(Debug, Clone)]
pub struct ChannelDescriptor(pub EngineType, pub EngineType, pub usize, pub usize);

#[derive(Debug)]
pub struct DataPathGraph {
    // the engines on the sender end for `tx_inputs` on each engine's DataPathNode
    // type of the engine, and the index in the sender engine's `tx_outputs`
    pub(crate) tx_inputs: HashMap<EngineType, Vec<(EngineType, usize)>>,
    // the engines on the receiver end for `tx_outputs` on each engine's DataPathNode
    // type of the engine, and the index in the sender engine's `tx_inputs`
    pub(crate) tx_outputs: HashMap<EngineType, Vec<(EngineType, usize)>>,
    // the engines on the sender end for `rx_inputs` on each engine's DataPathNode
    // type of the engine, and the index in the sender engine's `rx_outputs`
    pub(crate) rx_inputs: HashMap<EngineType, Vec<(EngineType, usize)>>,
    // the engines on the receiver end for `rx_outputs` on each engine's DataPathNode
    // type of the engine, and the index in the sender engine's `rx_inputs`
    pub(crate) rx_outputs: HashMap<EngineType, Vec<(EngineType, usize)>>,
}

impl DataPathGraph {
    pub(crate) fn new() -> Self {
        DataPathGraph {
            tx_inputs: HashMap::new(),
            tx_outputs: HashMap::new(),
            rx_inputs: HashMap::new(),
            rx_outputs: HashMap::new(),
        }
    }

    pub(crate) fn insert_node(
        &mut self,
        engine: EngineType,
        tx_inputs: Vec<(EngineType, usize)>,
        tx_outputs: Vec<(EngineType, usize)>,
        rx_inputs: Vec<(EngineType, usize)>,
        rx_outputs: Vec<(EngineType, usize)>,
    ) {
        self.tx_inputs.insert(engine.clone(), tx_inputs);
        self.tx_outputs.insert(engine.clone(), tx_outputs);
        self.rx_inputs.insert(engine.clone(), rx_inputs);
        self.rx_outputs.insert(engine, rx_outputs);
    }

    pub(crate) fn remove_node(&mut self, engine: &EngineType) {
        self.tx_inputs.remove(engine);
        self.tx_outputs.remove(engine);
        self.rx_inputs.remove(engine);
        self.rx_outputs.remove(engine);
    }
}
