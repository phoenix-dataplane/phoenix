//! Engine load balancer assigns engines to runtimes.

use crate::Engine;

pub(crate) trait EngineBalancer {
    fn submit(&self, engine: Box<dyn Engine>);
}