use std::pin::Pin;

use futures::future::BoxFuture;

pub mod manager;

pub(crate) mod lb;
pub(crate) mod runtime;
pub(crate) use runtime::Indicator;

pub(crate) mod graph;
pub(crate) use graph::{EngineRxMessage, RxIQueue, RxOQueue, TxIQueue, TxOQueue, Vertex};

pub(crate) mod upgradable;
pub(crate) use upgradable::{Upgradable, Version};

pub(crate) mod container;
pub(crate) use container::EngineContainer;

pub(crate) mod future;

pub(crate) mod channel;
pub(crate) mod flavors;

pub(crate) mod group;

pub(crate) trait Engine: Upgradable + Vertex + Send {
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
}

pub(crate) type EngineResult = Result<(), Box<dyn std::error::Error>>;

// NoProgress, MayDemandMoreCPU
// pub(crate) enum EngineStatus {
//     NoWork,
//     Continue,
//     Complete,
// }
