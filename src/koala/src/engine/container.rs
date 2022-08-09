use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;

use futures::future::BoxFuture;
use semver::Version;

use super::{Engine, EngineLocalStorage, EngineResult, EngineType, Indicator};

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
    ty: EngineType,
    version: Version,
    els: Option<&'static dyn EngineLocalStorage>,
}

/// Extending the future's lifetime.
/// SAFETY: Extremely unsafe!!!
#[inline]
unsafe fn extend_lifetime<'a>(
    fut: BoxFuture<'a, EngineResult>,
) -> BoxFuture<'static, EngineResult> {
    std::mem::transmute::<BoxFuture<'a, EngineResult>, BoxFuture<'static, EngineResult>>(fut)
}

impl EngineContainer {
    /// Spin up a new engine
    pub(crate) fn new(mut engine: Box<dyn Engine>, ty: EngineType, version: Version) -> Self {
        let indicator = Indicator::new(0);
        engine.set_tracker(indicator.clone());
        let desc = engine.description();
        // SAFETY: apparently EngineLocalStorage cannot be 'static,
        // The caller must ensure that the els() is called within an engine.
        // ENGINE_LS takes a 'static EngineLocalStorage to emulate a per-engine thread-local context.
        // It points to some states attatched to the engine
        // hence in reality its lifetime is bound by engine (future) s lifetime
        let els = unsafe { engine.els() };

        let mut pinned = Pin::new(engine);
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
            version,
            ty,
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
    pub(crate) unsafe fn els(&self) -> Option<&'static dyn EngineLocalStorage> {
        self.els
    }

    #[inline]
    pub(crate) fn set_els(&self) {
        self.engine.set_els();
    }

    #[inline]
    pub(crate) fn engine_type(&self) -> &EngineType {
        &self.ty
    }

    #[inline]
    pub(crate) fn version(&self) -> Version {
        self.version.clone()
    }

    /// Detach current engine in prepare for upgrade
    /// Some preparatory work is done during this step
    /// e.g., flush inter-engine shared queues
    /// There is no need to call this function if only moves
    pub(crate) fn detach(self) -> Box<dyn Engine> {
        std::mem::drop(self.future);
        let engine = self.engine;
        unsafe { Pin::into_inner_unchecked(engine) }
    }
}
