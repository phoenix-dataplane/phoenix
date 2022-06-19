use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;

use futures::future::BoxFuture;

use super::runtime::Indicator;
use super::{Engine, EngineLocalStorage, EngineResult};

pub(crate) struct EngineContainer {
    fut: BoxFuture<'static, EngineResult>,
    indicator: Indicator,
    desc: String,
    els: Option<&'static dyn EngineLocalStorage>,
}

impl EngineContainer {
    pub(crate) fn new<E: Engine>(mut engine: E) -> Self {
        let indicator = Indicator::new(0);
        engine.set_tracker(indicator.clone());

        let desc = engine.description();
        let els = unsafe { engine.els() };

        Self {
            fut: Box::pin(engine.entry()),
            indicator,
            desc,
            els,
        }
    }

    #[inline]
    pub(crate) fn future(&mut self) -> Pin<&mut dyn Future<Output = EngineResult>> {
        self.fut.as_mut()
    }

    #[inline]
    pub(crate) fn check_progress(&self) -> Indicator {
        Indicator::new(self.indicator.0.swap(0, Ordering::AcqRel))
    }

    #[inline]
    pub(crate) fn description(&self) -> &str {
        &self.desc
    }

    #[inline]
    pub(crate) unsafe fn els(&self) -> Option<&'static dyn EngineLocalStorage> {
        self.els
    }
}
