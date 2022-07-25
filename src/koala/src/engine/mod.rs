use std::future::Future;

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

pub(crate) trait Engine: Upgradable + Vertex + Send {
    /// The type of value produced on completion.
    type Future: Future<Output = EngineResult> + Send + 'static;

    /// Turn the Engine into an executable `Future`
    fn entry(self) -> Self::Future;

    /// Set the shared progress tracker.
    fn set_tracker(&mut self, indicator: Indicator);

    /// Returns a text description of the engine.
    fn description(&self) -> String;

    #[inline]
    unsafe fn els(&self) -> Option<&'static dyn EngineLocalStorage> {
        None
    }
}

pub(crate) type EngineResult = Result<(), Box<dyn std::error::Error>>;

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
pub(crate) unsafe trait EngineLocalStorage: std::any::Any + Send + Sync {
    fn as_any(&self) -> &dyn std::any::Any;
}

// pub(crate) trait AnySend: std::any::Any + Send {}
// pub(crate) struct EngineLocalStorage(pub(crate) &'static dyn AnySend);

// pub(crate) trait Engine: Upgradable + Send + Vertex {
//     /// `resume()` mush be non-blocking and short.
//     fn resume(&mut self) -> Result<EngineStatus, Box<dyn std::error::Error>>;
//     #[inline]
//     unsafe fn tls(&self) -> Option<&'static dyn std::any::Any> {
//         None
//     }
// }

// NoProgress, MayDemandMoreCPU
// pub(crate) enum EngineStatus {
//     NoWork,
//     Continue,
//     Complete,
// }
