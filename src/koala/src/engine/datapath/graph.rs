use std::collections::HashMap;

use petgraph::graph::NodeIndex;
use petgraph::visit::{Topo, Walker};
use petgraph::Graph;

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
#[derive(Debug, Clone, Copy)]
pub struct ChannelDescriptor(pub EngineType, pub EngineType, pub usize, pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FlowDirection {
    Tx,
    Rx,
}

struct FlowDependencyGraph {
    index: HashMap<(EngineType, FlowDirection), NodeIndex>,
    graph: Graph<(EngineType, FlowDirection), ()>,
}

impl FlowDependencyGraph {
    fn new() -> Self {
        FlowDependencyGraph {
            index: HashMap::new(),
            graph: Graph::new(),
        }
    }

    fn get_or_insert_index(&mut self, ty: EngineType, direction: FlowDirection) -> NodeIndex {
        let FlowDependencyGraph {
            ref mut index,
            ref mut graph,
        } = *self;
        *index
            .entry((ty, direction))
            .or_insert_with(|| graph.add_node((ty, direction)))
    }
}

#[derive(Debug)]
pub(crate) struct DataPathGraph {
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
        self.tx_inputs.insert(engine, tx_inputs);
        self.tx_outputs.insert(engine, tx_outputs);
        self.rx_inputs.insert(engine, rx_inputs);
        self.rx_outputs.insert(engine, rx_outputs);
    }

    #[allow(unused)]
    pub(crate) fn remove_node(&mut self, engine: &EngineType) {
        self.tx_inputs.remove(engine);
        self.tx_outputs.remove(engine);
        self.rx_inputs.remove(engine);
        self.rx_outputs.remove(engine);
    }

    pub(crate) fn topological_order(&self) -> Vec<(EngineType, FlowDirection)> {
        let mut graph = FlowDependencyGraph::new();
        for (engine_type, endpoints) in self.tx_outputs.iter() {
            let sender = graph.get_or_insert_index(*engine_type, FlowDirection::Tx);
            for (receiver, _) in endpoints.iter() {
                let receiver = graph.get_or_insert_index(*receiver, FlowDirection::Tx);
                graph.graph.add_edge(sender, receiver, ());
            }
        }
        for (engine_type, endpoints) in self.rx_outputs.iter() {
            let sender = graph.get_or_insert_index(*engine_type, FlowDirection::Rx);
            for (receiver, _) in endpoints.iter() {
                let receiver = graph.get_or_insert_index(*receiver, FlowDirection::Rx);
                graph.graph.add_edge(sender, receiver, ());
            }
        }

        if petgraph::algo::is_cyclic_directed(&graph.graph) {
            panic!("Data path graph contains a directed cycle")
        }

        let visit = Topo::new(&graph.graph);
        let topo_order_idx = visit.iter(&graph.graph).collect::<Vec<_>>();
        let topo_order = topo_order_idx
            .iter()
            .map(|node| graph.graph[*node])
            .collect();

        topo_order
    }
}
