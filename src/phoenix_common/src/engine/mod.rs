use std::os::unix::net::UCred;
use std::pin::Pin;

use futures::future::BoxFuture;

pub use crate::PhoenixResult;

pub mod future;

pub mod datapath;
pub use datapath::node::Vertex;

pub mod decompose;
pub use decompose::{Decompose, DecomposeResult};

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
    fn handle_request(&mut self, _request: Vec<u8>, _cred: UCred) -> PhoenixResult<()> {
        Ok(())
    }

    /// NOTE(wyj): temporary API
    /// engines should not have thread/runtime local states in the fugture
    /// Preform preparatory work before detaching the engine from runtime
    /// e.g., clean thread-local states
    #[inline]
    fn pre_detach(&mut self) -> PhoenixResult<()> {
        // empty default impl
        Ok(())
    }
}

/// This indicates the runtime of an engine's status.
#[derive(Debug)]
pub struct Indicator(pub(crate) usize);

impl Default for Indicator {
    fn default() -> Self {
        Self::new(0)
    }
}

#[allow(unused)]
impl Indicator {
    pub const BUSY: usize = usize::MAX;

    #[inline]
    pub(crate) fn new(x: usize) -> Self {
        Indicator(x)
    }

    #[inline]
    pub fn set_busy(&mut self) {
        self.0 = Self::BUSY;
    }

    #[inline]
    pub fn nwork(&self) -> usize {
        self.0
    }

    #[inline]
    pub fn set_nwork(&mut self, nwork: usize) {
        self.0 = nwork;
    }

    #[inline]
    pub(crate) fn is_busy(&self) -> bool {
        self.0 == Self::BUSY
    }

    #[inline]
    pub(crate) fn is_spinning(&self) -> bool {
        self.0 == 0
    }
}
