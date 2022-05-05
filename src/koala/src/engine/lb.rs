//! Engine load balancer assigns engines to runtimes.

use crate::engine::Engine;

pub(crate) trait EngineBalancer {
    /// Schedule a runtime for this engine.
    fn submit(&self, engine: Box<dyn Engine>);
}
