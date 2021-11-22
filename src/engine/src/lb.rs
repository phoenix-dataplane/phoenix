//! Engine load balancer assigns engines to runtimes.

use crate::Engine;

pub(crate) trait EngineBalancer {
    /// Schedule a runtime for this engine.
    fn submit(&self, engine: Box<dyn Engine>);
}
