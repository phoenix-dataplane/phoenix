//! Helper functions to NUMA-aware thread scheduling.

/// Expose functionalities provided by [libnuma].
///
/// [libnuma]: https://github.com/lemonrock/libnuma
#[doc(inline)]
pub use libnuma;

use libnuma::masks::indices::NodeIndex;

/// Returns the number of NUMA nodes we can use.
///
/// # Panics
///
/// Panics if [libnuma] is not properly initialized in advance.
pub fn num_numa_nodes() -> usize {
    if !libnuma::initialize() {
        panic!("NUMA is not available on the current system");
    }

    NodeIndex::number_of_nodes_permitted()
}

/// Conceptually equivalent to [`numa_run_on_node`].
///
/// It sets the cpumask to be all the cpus belonging to this `node_index`. Besides that, it also
/// sets the scheduling hint so that the backend runtime can make best scheduling decisions.
///
/// # Panics
///
/// - Panics if [libnuma] is not properly initialized in advance.
/// - Panics if the `node_index` is beyond number of permitted numa node.
/// - Panics if the call to the low-level function [`numa_run_on_node`] fails.
///
/// [`numa_run_on_node`]: https://linux.die.net/man/3/numa_run_on_node
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
