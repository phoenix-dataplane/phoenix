use std::collections::HashMap;
use std::iter::Iterator;

use petgraph::graph::NodeIndex;
use petgraph::visit::{DfsPostOrder, Walker};
use petgraph::Graph;
use thiserror::Error;

use crate::engine::{EnginePair, EngineType};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Engine type {:?} not found", .0)]
    EngineNotFound(EngineType),
}

pub(crate) struct EngineGraph {
    index: HashMap<EngineType, NodeIndex>,
    graph: Graph<EngineType, ()>,
}

impl EngineGraph {
    pub(crate) fn new() -> Self {
        EngineGraph {
            index: HashMap::new(),
            graph: Graph::new(),
        }
    }

    pub(crate) fn add_engines<I>(&mut self, engines: I)
    where
        I: IntoIterator<Item = EngineType>,
    {
        for engine in engines.into_iter() {
            let index = if let Some(index) = self.index.remove(&engine) {
                *self.graph.node_weight_mut(index).unwrap() = engine;
                index
            } else {
                self.graph.add_node(engine)
            };
            self.index.insert(engine, index);
        }
    }

    pub(crate) fn remove_dependency<I>(&mut self, edges: I) -> Result<(), Error>
    where
        I: IntoIterator<Item = EnginePair>,
    {
        for edge in edges {
            let from = *self
                .index
                .get(&edge.0)
                .ok_or(Error::EngineNotFound(edge.0))?;
            let to = *self
                .index
                .get(&edge.1)
                .ok_or(Error::EngineNotFound(edge.1))?;
            if let Some(edge_ix) = self.graph.find_edge(from, to) {
                self.graph.remove_edge(edge_ix);
            } else {
                log::warn!("no such edge from {:?} to {:?}", edge.0, edge.1);
            }
        }
        Ok(())
    }

    pub(crate) fn add_dependency<I>(&mut self, edges: I) -> Result<(), Error>
    where
        I: IntoIterator<Item = EnginePair>,
    {
        for edge in edges {
            let from = *self
                .index
                .get(&edge.0)
                .ok_or(Error::EngineNotFound(edge.0))?;
            let to = *self
                .index
                .get(&edge.1)
                .ok_or(Error::EngineNotFound(edge.1))?;
            self.graph.add_edge(from, to, ());
        }
        Ok(())
    }

    pub(crate) fn get_engine_dependencies(
        &self,
        service: &EngineType,
    ) -> Result<Vec<EngineType>, Error> {
        let service_idx = *self
            .index
            .get(service)
            .ok_or(Error::EngineNotFound(*service))?;
        let visit = DfsPostOrder::new(&self.graph, service_idx);
        let post_order = visit.iter(&self.graph).collect::<Vec<_>>();
        let rev_topo_order = post_order.iter().map(|node| self.graph[*node]).collect();
        Ok(rev_topo_order)
    }
}
