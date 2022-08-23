use std::os::unix::ucred::UCred;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

pub use anyhow::Result;

use futures::future::BoxFuture;

pub mod datapath;
pub mod decompose;
pub mod future;
pub mod manager;
pub(crate) mod lb;
pub(crate) mod container;
pub(crate) mod runtime;
pub(crate) mod upgrade;
pub(crate) use container::EngineContainer;

pub use decompose::Decompose;
pub use datapath::graph::Vertex;

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EngineType(pub &'static str);

pub type EnginePair = (EngineType, EngineType);

/// This indicates the runtime of an engine's status.
#[derive(Debug)]
pub struct Indicator(pub(crate) Arc<AtomicUsize>);

impl Clone for Indicator {
    fn clone(&self) -> Self {
        Indicator(Arc::clone(&self.0))
    }
}

impl Default for Indicator {
    fn default() -> Self {
        Self::new(0)
    }
}

pub trait Engine: Decompose + Send + Vertex + Unpin + 'static {
    /// Activate the engine, creates an executable `Future`
    /// This method takes a pinned pointer to the engine and returns a boxed future.
    /// TODO(wyj): double-check whether it is safe if the implmentation moves out the engine,
    /// (which can happend if the engine implements `Unpin`).
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult>;

    /// Set the shared progress tracker.
    fn set_tracker(&mut self, indicator: Indicator);

    /// Returns a text description of the engine.
    fn description(&self) -> String;

    #[inline]
    fn set_els(&self) { }

    #[inline]
    fn handle_request(&mut self, _request: Vec<u8>, _cred: UCred) -> Result<()> {
        Ok(())
    }
}

pub type EngineResult = Result<(), Box<dyn std::error::Error>>;
