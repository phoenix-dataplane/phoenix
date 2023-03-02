use std::collections::HashMap;

use petgraph::graph::NodeIndex;
use petgraph::visit::{Topo, Walker};
use petgraph::Graph;
use thiserror::Error;

use phoenix_common::engine::datapath::{
    create_channel, ChannelDescriptor, ChannelFlavor, DataPathNode, RxIQueue, RxOQueue, TxIQueue,
    TxOQueue,
};
use phoenix_common::engine::EngineType;

use super::group::GroupUnionFind;
use crate::tracing;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Endpoint index is not contiguous, index={0}")]
    IndexNotContiguous(usize),
    #[error("Invalid replacement {0:?}")]
    InvalidReplacement(ChannelDescriptor),
    #[error("Addon engine {0:?} not found")]
    AddonNotFound(EngineType),
    #[error("Channel to be replaced is not empty, receiver_engine={0:?}, endpoint=({1:?}, {2})")]
    ChannelNotEmpty(EngineType, EndpointType, usize),
    #[error("Engine {0:?}'s channels ({1:?}) and the graph descriptor mismatched")]
    NodeTampered(EngineType, EndpointType),
    #[error("Dangling endpoints left after replacement")]
    DanglingEndpoint,
}

#[derive(Debug, Clone, Copy)]
pub enum EndpointType {
    TxInput,
    TxOutput,
    RxInput,
    RxOutput,
}

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

// create a set of `DataPathNode`s for a service engine group
pub(crate) fn create_datapath_channels<I>(
    tx_edges: I,
    rx_edges: I,
    // engines that should be put in the same scheduling group
    groups: &GroupUnionFind,
) -> Result<(HashMap<EngineType, DataPathNode>, DataPathGraph), Error>
where
    I: IntoIterator<Item = ChannelDescriptor>,
{
    let mut endpoints = HashMap::new();
    for edge in tx_edges {
        let (sender, receiver) = if groups.is_same_group(edge.0, edge.1) {
            tracing::debug!(
                "Creating sequential channel between {:?} and {:?}",
                edge.0,
                edge.1
            );
            create_channel(ChannelFlavor::Sequential)
        } else {
            tracing::debug!(
                "Creating concurrent channel between {:?} and {:?}",
                edge.0,
                edge.1
            );
            create_channel(ChannelFlavor::Concurrent)
        };
        let sender_endpoint = endpoints
            .entry(edge.0)
            .or_insert_with(EndpointCollection::new);
        sender_endpoint
            .tx_outputs
            .push((edge.1, edge.3, sender, edge.2));
        let receiver_endpoint = endpoints
            .entry(edge.1)
            .or_insert_with(EndpointCollection::new);
        receiver_endpoint
            .tx_inputs
            .push((edge.0, edge.2, receiver, edge.3));
    }
    for edge in rx_edges {
        let (sender, receiver) = if groups.is_same_group(edge.0, edge.1) {
            create_channel(ChannelFlavor::Sequential)
        } else {
            create_channel(ChannelFlavor::Concurrent)
        };
        let sender_endpoint = endpoints
            .entry(edge.0)
            .or_insert_with(EndpointCollection::new);
        sender_endpoint
            .rx_outputs
            .push((edge.1, edge.3, sender, edge.2));
        let receiver_endpoint = endpoints
            .entry(edge.1)
            .or_insert_with(EndpointCollection::new);
        receiver_endpoint
            .rx_inputs
            .push((edge.0, edge.2, receiver, edge.3));
    }

    let mut nodes = HashMap::with_capacity(endpoints.len());
    let mut graph = DataPathGraph::new();
    for (engine, endpoint) in endpoints.into_iter() {
        let (node, endpoint_info) = endpoint.create_node()?;
        let [tx_inputs, tx_outputs, rx_inputs, rx_outputs] = endpoint_info;
        nodes.insert(engine, node);
        graph.insert_node(engine, tx_inputs, tx_outputs, rx_inputs, rx_outputs);
    }

    Ok((nodes, graph))
}

pub(crate) struct EndpointCollection {
    // engine on the other endpoint (sender), index in its tx_outputs
    // receiver, index in tx_inputs
    pub(crate) tx_inputs: Vec<(EngineType, usize, TxIQueue, usize)>,
    // engine on the other endpoint, index in its tx_inputs
    // sender, index in tx_outputs
    pub(crate) tx_outputs: Vec<(EngineType, usize, TxOQueue, usize)>,
    // engine on the other endpoint, index in its rx_outputs
    // receiver, index in rx_inputs
    pub(crate) rx_inputs: Vec<(EngineType, usize, RxIQueue, usize)>,
    // engine on the other endpoint, index in its rx_inputs
    // sender, index in rx_outputs
    pub(crate) rx_outputs: Vec<(EngineType, usize, RxOQueue, usize)>,
}

impl EndpointCollection {
    pub(crate) fn new() -> Self {
        EndpointCollection {
            tx_inputs: Vec::new(),
            tx_outputs: Vec::new(),
            rx_inputs: Vec::new(),
            rx_outputs: Vec::new(),
        }
    }

    // create DataPathNode and insert the endpoint information into graph
    #[allow(clippy::type_complexity)]
    pub(crate) fn create_node(
        mut self,
    ) -> Result<(DataPathNode, [Vec<(EngineType, usize)>; 4]), Error> {
        self.tx_inputs.sort_by_key(|x| x.3);
        self.tx_outputs.sort_by_key(|x| x.3);
        self.rx_inputs.sort_by_key(|x| x.3);
        self.rx_outputs.sort_by_key(|x| x.3);

        let mut tx_receivers = Vec::with_capacity(self.tx_inputs.len());
        let mut tx_inputs_engines = Vec::with_capacity(self.tx_inputs.len());
        for (sender_engine, sender_index, receiver, index) in self.tx_inputs {
            if index != tx_receivers.len() {
                return Err(Error::IndexNotContiguous(index));
            }
            tx_receivers.push(receiver);
            tx_inputs_engines.push((sender_engine, sender_index));
        }

        let mut tx_senders = Vec::with_capacity(self.tx_outputs.len());
        let mut tx_outputs_engines = Vec::with_capacity(self.tx_outputs.len());
        for (receiver_engine, receiver_index, sender, index) in self.tx_outputs {
            if index != tx_senders.len() {
                return Err(Error::IndexNotContiguous(index));
            }
            tx_senders.push(sender);
            tx_outputs_engines.push((receiver_engine, receiver_index));
        }

        let mut rx_receivers = Vec::with_capacity(self.rx_inputs.len());
        let mut rx_inputs_engines = Vec::with_capacity(self.rx_inputs.len());
        for (sender_engine, sender_index, receiver, index) in self.rx_inputs {
            if index != rx_receivers.len() {
                return Err(Error::IndexNotContiguous(index));
            }
            rx_receivers.push(receiver);
            rx_inputs_engines.push((sender_engine, sender_index));
        }

        let mut rx_senders = Vec::with_capacity(self.rx_outputs.len());
        let mut rx_outputs_engines = Vec::with_capacity(self.rx_outputs.len());
        for (receiver_engine, receiver_index, sender, index) in self.rx_outputs {
            if index != rx_senders.len() {
                return Err(Error::IndexNotContiguous(index));
            }
            rx_senders.push(sender);
            rx_outputs_engines.push((receiver_engine, receiver_index));
        }

        let node = DataPathNode {
            tx_inputs: tx_receivers,
            tx_outputs: tx_senders,
            rx_inputs: rx_receivers,
            rx_outputs: rx_senders,
        };

        let endpoint_info = [
            tx_inputs_engines,
            tx_outputs_engines,
            rx_inputs_engines,
            rx_outputs_engines,
        ];

        Ok((node, endpoint_info))
    }
}
