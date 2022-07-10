use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;

use futures::future::BoxFuture;

use super::runtime::Indicator;
use super::{Engine, EngineLocalStorage, EngineResult};

pub(crate) struct DetachedEngineContainer {
    engine: Box<dyn Engine>,
    desc: String,
}

pub(crate) struct EngineContainer {
    // SAFETY: the future's lifetime will be extended to static.
    // We must ensure the engine is not moved / dropped 
    // before polling the future.
    future: BoxFuture<'static, EngineResult>,
    // SAFETY: The engine itself can be `Unpin`. 
    // But after the future is created from the the engine  
    // it should not be moved. Otherwise, the mutable reference
    // hold in future is no longer valid.
    engine: Pin<Box<dyn Engine>>,
    indicator: Indicator,
    desc: String,
    els: Option<&'static dyn EngineLocalStorage>,
}

/// Extending the future's lifetime.
/// SAFETY: Extremely unsafe!!! 
#[inline]
unsafe fn extend_lifetime<'a>(fut: BoxFuture<'a, EngineResult>) -> BoxFuture<'static, EngineResult> {
    std::mem::transmute::<BoxFuture<'a, EngineResult>, BoxFuture<'static, EngineResult>>(fut)
}

impl EngineContainer {
    pub(crate) fn new<E: Engine>(mut engine: E) -> Self {
        let indicator = Indicator::new(0);
        engine.set_tracker(indicator.clone());
        let desc = engine.description();
        // SAFETY: apparently EngineLocalStorage cannot be 'static,
        // The caller must ensure that the els() is called within an engine.
        // ENGINE_LS takes a 'static EngineLocalStorage to emulate a per-engine thread-local context.
        // It points to some states attatched to the engine
        // hence in reality its lifetime is bound by engine (future) s lifetime
        let els = unsafe { engine.els() };

        let mut pinned = Box::pin(engine);
        let future = {
            let engine_mut = pinned.as_mut();
            // SAFETY: manaually extend 
            let fut = engine_mut.activate();
            unsafe { extend_lifetime(fut) }
        };

        Self {
            future,
            engine: pinned,
            indicator,
            desc,
            els,
        }
    }

    #[inline]
    pub(crate) fn future(&mut self) -> Pin<&mut dyn Future<Output = EngineResult>> {
        self.future.as_mut()
    }

    #[inline]
    pub(crate) fn with_indicator<T, F: FnOnce(&Indicator) -> T>(&self, f: F) -> T {
        let ret = f(&self.indicator);
        self.indicator.0.store(0, Ordering::Release);
        ret
    }

    #[inline]
    pub(crate) fn description(&self) -> &str {
        &self.desc
    }

    #[inline]
    pub(crate) fn suspend(self) -> Susp

    #[inline]
    pub(crate) unsafe fn els(&self) -> Option<&'static dyn EngineLocalStorage> {
        self.els
    }
}
