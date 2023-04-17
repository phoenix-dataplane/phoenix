use super::datapath::node::DataPathNode;
use crate::storage::{ResourceCollection, SharedStorage};

pub type DecomposeResult<T> = anyhow::Result<T>;

pub trait Decompose {
    /// Perform preparatory work before decompose the engine,
    /// e.g., flush data and command queues
    fn flush(&mut self) -> DecomposeResult<usize>;

    /// Decompose the engines to compositional states,
    /// and extract the data path node
    fn decompose(
        self: Box<Self>,
        shared: &mut SharedStorage,
        global: &mut ResourceCollection,
    ) -> (ResourceCollection, DataPathNode);
}
