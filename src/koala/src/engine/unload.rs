use crate::storage::{ResourceCollection, SharedStorage};
use super::datapath::node::DataPathNode;

pub trait Unload {
    /// Perform preparatory work before decompose the engine,
    /// e.g., flush data and command queues 
    fn detach(&mut self);

    /// Decompose the engines to compositional states,
    /// and extract the data path node
    fn unload(
        self: Box<Self>,
        shared: &mut SharedStorage,
        global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode);
}
