use std::collections::{HashMap, HashSet};

use thiserror::Error;

use super::channel::{create_channel, ChannelFlavor};
use super::graph::{ChannelDescriptor, DataPathGraph};
use super::graph::{RxIQueue, RxOQueue, TxIQueue, TxOQueue};
use crate::engine::group::{GroupId, GroupUnionFind};
use crate::engine::{Engine, EngineType};

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

pub struct DataPathNode {
    pub tx_inputs: Vec<TxIQueue>,
    pub tx_outputs: Vec<TxOQueue>,
    pub rx_inputs: Vec<RxIQueue>,
    pub rx_outputs: Vec<RxOQueue>,
}

impl DataPathNode {
    pub(crate) fn new() -> Self {
        DataPathNode {
            tx_inputs: Vec::new(),
            tx_outputs: Vec::new(),
            rx_inputs: Vec::new(),
            rx_outputs: Vec::new(),
        }
    }
}

struct EndpointCollection {
    // engine on the other endpoint (sender), index in its tx_outputs
    // receiver, index in tx_inputs
    tx_inputs: Vec<(EngineType, usize, TxIQueue, usize)>,
    // engine on the other endpoint, index in its tx_inputs
    // sender, index in tx_outputs
    tx_outputs: Vec<(EngineType, usize, TxOQueue, usize)>,
    // engine on the other endpoint, index in its rx_outputs
    // receiver, index in rx_inputs
    rx_inputs: Vec<(EngineType, usize, RxIQueue, usize)>,
    // engine on the other endpoint, index in its rx_inputs
    // sender, index in rx_outputs
    rx_outputs: Vec<(EngineType, usize, RxOQueue, usize)>,
}

impl EndpointCollection {
    fn new() -> Self {
        EndpointCollection {
            tx_inputs: Vec::new(),
            tx_outputs: Vec::new(),
            rx_inputs: Vec::new(),
            rx_outputs: Vec::new(),
        }
    }

    // create DataPathNode and insert the endpoint information into graph
    fn create_node(mut self) -> Result<(DataPathNode, [Vec<(EngineType, usize)>; 4]), Error> {
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

// Replace senders/receivers on the data path to install an addon
// * group: which scheduling group should the addon belongs to
pub(crate) fn refactor_channels_attach_addon<I>(
    engines: &mut HashMap<EngineType, Box<dyn Engine>>,
    graph: &mut DataPathGraph,
    addon: EngineType,
    tx_edges_replacement: I,
    rx_edges_replacement: I,
    group: &HashSet<EngineType>,
) -> Result<DataPathNode, Error>
where
    I: IntoIterator<Item = ChannelDescriptor>,
{
    let mut addon_endpoint = EndpointCollection::new();

    let mut senders_await_replace = HashSet::new();
    let mut receivers_await_replace = HashSet::new();
    for edge in tx_edges_replacement.into_iter() {
        if edge.0 == addon {
            // newly installed addon is the sender
            let receiver_endpoint = engines
                .get_mut(&edge.1)
                .ok_or(Error::InvalidReplacement(edge))?;
            let receiver_tx_inputs = graph.tx_inputs.get_mut(&edge.1).unwrap();
            if edge.3 >= receiver_tx_inputs.len() {
                return Err(Error::InvalidReplacement(edge));
            }
            if receiver_tx_inputs.len() != receiver_endpoint.tx_inputs().len() {
                return Err(Error::NodeTampered(edge.1, EndpointType::TxInput));
            }
            if !receivers_await_replace.remove(&(edge.1, edge.3)) {
                // original channel's sender end has not be replaced yet
                // we must check that the sender end will be replaced at a later stage
                // otherwise, sender end must have already been replaced.
                senders_await_replace.insert(receiver_tx_inputs[edge.3]);
            }
            if !receiver_endpoint.tx_inputs()[edge.3].is_empty() {
                return Err(Error::ChannelNotEmpty(
                    edge.1,
                    EndpointType::TxInput,
                    edge.3,
                ));
            }
            let (sender, receiver) = if group.contains(&edge.1) {
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
            // replace the sender and receiver
            receiver_tx_inputs[edge.3] = (edge.0, edge.2);
            receiver_endpoint.tx_inputs()[edge.3] = receiver;
            addon_endpoint
                .tx_outputs
                .push((edge.1, edge.3, sender, edge.2));
        } else if edge.1 == addon {
            // new addon is the receiver
            let sender_endpoint = engines
                .get_mut(&edge.0)
                .ok_or(Error::InvalidReplacement(edge))?;
            let sender_tx_outputs = graph.tx_outputs.get_mut(&edge.0).unwrap();
            if edge.2 >= sender_tx_outputs.len() {
                return Err(Error::InvalidReplacement(edge));
            }
            if sender_tx_outputs.len() != sender_endpoint.tx_outputs().len() {
                return Err(Error::NodeTampered(edge.0, EndpointType::TxOutput));
            }
            if !senders_await_replace.remove(&(edge.0, edge.2)) {
                receivers_await_replace.insert(sender_tx_outputs[edge.2]);
            }
            let (sender, receiver) = if group.contains(&edge.0) {
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
            sender_tx_outputs[edge.2] = (edge.1, edge.3);
            sender_endpoint.tx_outputs()[edge.2] = sender;
            addon_endpoint
                .tx_inputs
                .push((edge.0, edge.2, receiver, edge.3));
        } else {
            return Err(Error::InvalidReplacement(edge));
        }
    }
    if !senders_await_replace.is_empty() || !receivers_await_replace.is_empty() {
        return Err(Error::DanglingEndpoint);
    }

    for edge in rx_edges_replacement.into_iter() {
        if edge.0 == addon {
            // new plugin is the sender
            let receiver_endpoint = engines
                .get_mut(&edge.1)
                .ok_or(Error::InvalidReplacement(edge))?;
            let receiver_rx_inputs = graph.rx_inputs.get_mut(&edge.1).unwrap();
            if edge.3 >= receiver_rx_inputs.len() {
                return Err(Error::InvalidReplacement(edge));
            }
            if receiver_rx_inputs.len() != receiver_endpoint.rx_inputs().len() {
                return Err(Error::NodeTampered(edge.1, EndpointType::RxInput));
            }
            if !receivers_await_replace.remove(&(edge.1, edge.3)) {
                // original channel's sender end has not be replaced yet
                // we must check that the sender end will be replaced at a later stage
                // otherwise, sender end must have already been replaced.
                senders_await_replace.insert(receiver_rx_inputs[edge.3]);
            }
            if !receiver_endpoint.rx_inputs()[edge.3].is_empty() {
                return Err(Error::ChannelNotEmpty(
                    edge.1,
                    EndpointType::RxInput,
                    edge.3,
                ));
            }
            let (sender, receiver) = if group.contains(&edge.1) {
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
            receiver_rx_inputs[edge.3] = (edge.0, edge.2);
            receiver_endpoint.rx_inputs()[edge.3] = receiver;
            addon_endpoint
                .rx_outputs
                .push((edge.1, edge.3, sender, edge.2));
        } else if edge.1 == addon {
            // new plugin is the receiver
            let sender_endpoint = engines
                .get_mut(&edge.0)
                .ok_or(Error::InvalidReplacement(edge))?;
            let sender_rx_outputs = graph.rx_outputs.get_mut(&edge.0).unwrap();
            if edge.2 >= sender_rx_outputs.len() {
                return Err(Error::InvalidReplacement(edge));
            }
            if sender_rx_outputs.len() != sender_endpoint.rx_outputs().len() {
                return Err(Error::NodeTampered(edge.0, EndpointType::RxOutput));
            }
            if !senders_await_replace.remove(&(edge.0, edge.2)) {
                receivers_await_replace.insert(sender_rx_outputs[edge.2]);
            }
            let (sender, receiver) = if group.contains(&edge.0) {
                create_channel(ChannelFlavor::Sequential)
            } else {
                create_channel(ChannelFlavor::Concurrent)
            };
            sender_rx_outputs[edge.2] = (edge.1, edge.3);
            sender_endpoint.rx_outputs()[edge.2] = sender;
            addon_endpoint
                .rx_inputs
                .push((edge.0, edge.2, receiver, edge.3));
        } else {
            return Err(Error::InvalidReplacement(edge));
        }
    }
    if !senders_await_replace.is_empty() || !receivers_await_replace.is_empty() {
        return Err(Error::DanglingEndpoint);
    }

    let (node, endpoint_info) = addon_endpoint.create_node()?;
    let [tx_inputs, tx_outputs, rx_inputs, rx_outputs] = endpoint_info;
    graph.insert_node(addon, tx_inputs, tx_outputs, rx_inputs, rx_outputs);

    Ok(node)
}

///
pub(crate) fn refactor_channels_detach_addon<I>(
    engines: &mut HashMap<EngineType, (Box<dyn Engine>, GroupId)>,
    graph: &mut DataPathGraph,
    addon: EngineType,
    tx_edges_replacement: I,
    rx_edges_replacement: I,
) -> Result<(), Error>
where
    I: IntoIterator<Item = ChannelDescriptor>,
{
    let (mut addon_engine, _) = engines.remove(&addon).ok_or(Error::AddonNotFound(addon))?;
    let tx_inputs_len = graph.tx_inputs.get_mut(&addon).unwrap().len();
    let tx_outputs_len = graph.tx_outputs.get_mut(&addon).unwrap().len();
    let rx_inputs_len = graph.rx_inputs.get_mut(&addon).unwrap().len();
    let rx_outputs_len = graph.rx_outputs.get_mut(&addon).unwrap().len();
    if addon_engine.tx_inputs().len() != tx_inputs_len {
        return Err(Error::NodeTampered(addon, EndpointType::TxInput));
    }
    if addon_engine.tx_outputs().len() != tx_outputs_len {
        return Err(Error::NodeTampered(addon, EndpointType::TxOutput));
    }
    if addon_engine.rx_inputs().len() != rx_inputs_len {
        return Err(Error::NodeTampered(addon, EndpointType::RxInput));
    }
    if addon_engine.rx_outputs().len() != rx_outputs_len {
        return Err(Error::NodeTampered(addon, EndpointType::RxOutput));
    }

    let mut tx_inputs_await_replace = (0..tx_inputs_len).collect::<HashSet<_>>();
    let mut tx_outputs_await_replace = (0..tx_outputs_len).collect::<HashSet<_>>();
    for edge in tx_edges_replacement.into_iter() {
        let sender_group = engines
            .get(&edge.0)
            .ok_or(Error::InvalidReplacement(edge))?
            .1;
        let receiver_group = engines
            .get(&edge.1)
            .ok_or(Error::InvalidReplacement(edge))?
            .1;
        let (sender, receiver) = if sender_group == receiver_group {
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

        let (sender_endpoint, _) = engines.get_mut(&edge.0).unwrap();
        let sender_tx_outputs = graph.tx_outputs.get_mut(&edge.0).unwrap();
        if edge.2 >= sender_tx_outputs.len() || sender_tx_outputs[edge.2].0 != addon {
            return Err(Error::InvalidReplacement(edge));
        }
        if sender_tx_outputs.len() != sender_endpoint.tx_outputs().len() {
            return Err(Error::NodeTampered(edge.0, EndpointType::TxOutput));
        }
        let receiver_index = sender_tx_outputs[edge.2].1;
        if !addon_engine.tx_inputs()[receiver_index].is_empty() {
            return Err(Error::ChannelNotEmpty(
                addon,
                EndpointType::TxInput,
                receiver_index,
            ));
        }
        tx_inputs_await_replace.remove(&receiver_index);
        sender_tx_outputs[edge.2] = (edge.1, edge.3);
        sender_endpoint.tx_outputs()[edge.2] = sender;

        let (receiver_endpoint, _) = engines.get_mut(&edge.1).unwrap();
        let receiver_tx_inputs = graph.tx_inputs.get_mut(&edge.1).unwrap();
        if edge.3 >= receiver_tx_inputs.len() || receiver_tx_inputs[edge.3].0 != addon {
            return Err(Error::InvalidReplacement(edge));
        }
        if !receiver_endpoint.tx_inputs()[edge.3].is_empty() {
            return Err(Error::ChannelNotEmpty(
                edge.1,
                EndpointType::TxInput,
                edge.3,
            ));
        }
        tx_outputs_await_replace.remove(&receiver_tx_inputs[edge.3].1);
        receiver_tx_inputs[edge.3] = (edge.0, edge.2);
        receiver_endpoint.tx_inputs()[edge.3] = receiver;
    }
    if !tx_inputs_await_replace.is_empty() || !tx_outputs_await_replace.is_empty() {
        return Err(Error::DanglingEndpoint);
    }

    let mut rx_inputs_await_replace = (0..rx_inputs_len).collect::<HashSet<_>>();
    let mut rx_outputs_await_replace = (0..rx_outputs_len).collect::<HashSet<_>>();
    for edge in rx_edges_replacement.into_iter() {
        let sender_group = engines
            .get(&edge.0)
            .ok_or(Error::InvalidReplacement(edge))?
            .1;
        let receiver_group = engines
            .get(&edge.1)
            .ok_or(Error::InvalidReplacement(edge))?
            .1;
        let (sender, receiver) = if sender_group == receiver_group {
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

        let (sender_endpoint, _) = engines.get_mut(&edge.0).unwrap();
        let sender_rx_outputs = graph.rx_outputs.get_mut(&edge.0).unwrap();
        if edge.2 >= sender_rx_outputs.len() || sender_rx_outputs[edge.2].0 != addon {
            return Err(Error::InvalidReplacement(edge));
        }
        if sender_rx_outputs.len() != sender_endpoint.rx_outputs().len() {
            return Err(Error::NodeTampered(edge.0, EndpointType::RxOutput));
        }
        let receiver_index = sender_rx_outputs[edge.2].1;
        if !addon_engine.rx_inputs()[receiver_index].is_empty() {
            return Err(Error::ChannelNotEmpty(
                addon,
                EndpointType::RxInput,
                receiver_index,
            ));
        }
        rx_inputs_await_replace.remove(&receiver_index);
        sender_rx_outputs[edge.2] = (edge.1, edge.3);
        sender_endpoint.rx_outputs()[edge.2] = sender;

        let (receiver_endpoint, _) = engines.get_mut(&edge.1).unwrap();
        let receiver_rx_inputs = graph.rx_inputs.get_mut(&edge.1).unwrap();
        if edge.3 >= receiver_rx_inputs.len() || receiver_rx_inputs[edge.3].0 != addon {
            return Err(Error::InvalidReplacement(edge));
        }
        if !receiver_endpoint.rx_inputs()[edge.3].is_empty() {
            return Err(Error::ChannelNotEmpty(
                edge.1,
                EndpointType::RxInput,
                edge.3,
            ));
        }
        rx_outputs_await_replace.remove(&receiver_rx_inputs[edge.3].1);
        receiver_rx_inputs[edge.3] = (edge.0, edge.2);
        receiver_endpoint.rx_inputs()[edge.3] = receiver;
    }
    if !rx_inputs_await_replace.is_empty() || !rx_outputs_await_replace.is_empty() {
        return Err(Error::DanglingEndpoint);
    }

    graph.remove_node(&addon);
    Ok(())
}
