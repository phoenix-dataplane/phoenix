use std::collections::HashMap;
use std::iter::Iterator;

use petgraph::graph::NodeIndex;
use petgraph::visit::{DfsPostOrder, Walker, WalkerIter};
use petgraph::Graph;

use crate::engine::{EnginePair, EngineType};

pub struct EngineGraph {
    index: HashMap<EngineType, NodeIndex>,
    graph: Graph<EngineType, ()>,
}

impl EngineGraph {
    pub fn new() -> Self {
        EngineGraph {
            index: HashMap::new(),
            graph: Graph::new(),
        }
    }

    fn get_or_insert_index(&mut self, engine: &EngineType) -> NodeIndex {
        let EngineGraph {
            ref mut index,
            ref mut graph,
        } = *self;
        *index
            .entry(engine.clone())
            .or_insert_with(|| graph.add_node(engine.clone()))
    }

    pub fn add_dependency(&mut self, edges: &[EnginePair]) {
        for edge in edges {
            let from = self.get_or_insert_index(&edge.0);
            let to = self.get_or_insert_index(&edge.1);
            self.graph.add_edge(from, to, ());
        }
    }

    pub fn get_engine_dependencies(&self, service: &EngineType) -> Vec<EngineType> {
        let visit = DfsPostOrder::new(&self.graph, self.index[service]);
        let post_order = visit.iter(&self.graph).collect::<Vec<_>>();
        let topo_order = post_order
            .iter()
            .rev()
            .map(|node| self.graph[*node].clone())
            .collect();
        topo_order
    }
}
