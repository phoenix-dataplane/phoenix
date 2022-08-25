use std::os::unix::ucred::UCred;
use std::pin::Pin;

pub use anyhow::Result;
use futures::future::BoxFuture;

pub(crate) mod container;
pub(crate) use container::EngineContainer;

pub mod future;
pub mod manager;

pub mod datapath;
pub use datapath::graph::Vertex;

pub mod decompose;
pub use decompose::Decompose;

pub(crate) mod group;
pub(crate) mod lb;

pub(crate) mod runtime;
pub use runtime::Indicator;

pub(crate) mod upgrade;
pub(crate) use group::SchedulingGroup;

pub type EngineResult = Result<(), Box<dyn std::error::Error>>;

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EngineType(pub &'static str);

pub type EnginePair = (EngineType, EngineType);

pub trait Engine: Decompose + Send + Vertex + Unpin + 'static {
    /// Turn the Engine into an executable `Future`
    fn activate<'a>(self: Pin<&'a mut Self>) -> BoxFuture<'a, EngineResult>;

    /// Returns a text description of the engine.
    fn description(self: Pin<&Self>) -> String;

    /// Returns the progress tracker, which implies the future work.
    fn tracker(self: Pin<&mut Self>) -> &mut Indicator;

    /// Asks the engine to updates its local storage pointer.
    ///
    /// # Warning
    ///
    /// EngineLocalStorage is only accessed from a thread/runtime at a time. There should be no
    /// concurrent access to it. But since Engine can be moved between Runtimes, the local storage
    /// could be read from different threads _at different times_ (i.e., _Send_).
    ///
    /// The user must ensure their storage type are _Send_.
    #[inline]
    fn set_els(self: Pin<&mut Self>) {
        // empty default impl
    }

    /// Handle request sent by the network operator.
    #[inline]
    fn handle_request(&mut self, _request: Vec<u8>, _cred: UCred) -> Result<()> {
        Ok(())
    }
}