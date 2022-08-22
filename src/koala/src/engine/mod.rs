use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

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

pub use runtime::ENGINE_LS;

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
    unsafe fn els(&self) -> Option<&'static dyn EngineLocalStorage> {
        None
    }

    fn set_els(&self) {}
}

pub type EngineResult = Result<(), Box<dyn std::error::Error>>;

/// Safety: EngineLocalStorage is only accessed from a thread/runtime at a time. There should be no
/// concurrent access to it. But since Engine can be moved between Runtimes, the local storage
/// could be read from different threads _at different times_ (i.e., Send).
///
/// The user must ensure their storage type are Send.
///
/// WARNING(cjr): EngineLocalStorage is Sync only because runtime is shared between threads, and
/// they require it to be Sync. They are, in fact, not concurrently accessed, so we even if the
/// underlying data is not threadsafe, we are still good here.
///
/// TODO(cjr): Consider remove this EngineLocalStorage when ShmPtr is ready.
pub unsafe trait EngineLocalStorage: std::any::Any + Send + Sync {
    fn as_any(&self) -> &dyn std::any::Any;
}
