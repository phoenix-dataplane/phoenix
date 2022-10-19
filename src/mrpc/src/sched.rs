pub use libnuma;

use libnuma::masks::indices::NodeIndex;

pub fn num_numa_nodes() -> usize {
    if !libnuma::initialize() {
        panic!("NUMA is not available on the current system");
    }

    NodeIndex::number_of_nodes_permitted()
}

pub fn bind_to_node(node_index: u8) {
    if !libnuma::initialize() {
        panic!("NUMA is not available on the current system");
    }

    let num_numa_nodes = NodeIndex::number_of_nodes_permitted();
    assert!(
        (node_index as usize) < num_numa_nodes,
        "node_index: {} vs num_numa_nodes: {}",
        node_index,
        num_numa_nodes
    );

    let numa_node_to_use = NodeIndex::new(node_index);
    numa_node_to_use.run_current_thread_on_this();

    let mut hint = crate::get_schedulint_hint();
    hint.numa_node_affinity = Some(node_index);
    crate::set_schedulint_hint(&hint);
}
