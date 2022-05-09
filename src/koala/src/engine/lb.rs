//! Engine load balancer assigns engines to runtimes.

use super::Engine;

pub(crate) trait EngineBalancer {
    /// Schedule a runtime for this engine.
    fn submit<E: Engine>(&self, engine: E);
}
