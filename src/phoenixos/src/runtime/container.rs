use std::future::Future;
use std::os::unix::ucred::UCred;
use std::pin::Pin;

use futures::future::BoxFuture;
use semver::Version;

use phoenix_common::engine::{Engine, EngineResult, EngineType};

/// A container that bundles a `Box<dyn Engine>` and its `Future` object so that the caller of this
/// type can use both the methods provided by the `Engine` trait and poll the future.
pub(crate) struct EngineContainer {
    /// The `Future` object of the contained `engine`.
    ///
    /// # Safety
    ///
    /// We pretend the lifetime of this object to be static, but in fact, the lifetime should
    /// have been bounded to the `engine` object `EngineContainer`. The user of this type
    /// (i.e., `Runtime`) must ensure the `engine` object is neither moved nor dropped
    /// when the future is not ready.
    future: BoxFuture<'static, EngineResult>,

    /// The Engine trait object. The engine itself can be `Unpin`, but after the future object
    /// is created from the engine (by calling its `activate` method), the object can longer be
    /// moved (and this is done by converting the object into a
    /// [pinned pointer](https://doc.rust-lang.org/std/pin/index.html)).
    ///
    /// # Warning
    ///
    /// The user of this `EngineContainer` must ensure the engine is not dropped or moved
    /// (impossible with pinned Box) while the future is still in use (not returning
    /// `Pending::Ready`).
    engine: Pin<Box<dyn Engine>>,

    /// The type of the engine.
    ty: EngineType,

    /// The verion of the phoenix module that the engine belongs to.
    version: Version,
}

/// Extending the future's lifetime from 'a to 'static.
///
/// # Safety
/// Use this function at your own risk.
#[inline]
unsafe fn extend_lifetime<'a>(
    fut: BoxFuture<'a, EngineResult>,
) -> BoxFuture<'static, EngineResult> {
    std::mem::transmute::<BoxFuture<'a, EngineResult>, BoxFuture<'static, EngineResult>>(fut)
}

impl EngineContainer {
    pub(crate) fn new(engine: Box<dyn Engine>, ty: EngineType, version: Version) -> Self {
        let mut pinned = Pin::new(engine);
        let future = {
            let fut = pinned.as_mut().activate();
            // SAFETY: In Rust, we cannot inform the compiler that the future reference to the
            // Engine in the same struct, this pattern cannot be described with the usual
            // borrowing rules. Therefore, we make the engine pinned and pretend the lifetime of
            // the future is 'static. This is fine as long as we make sure engine is dropped later
            // than the future.
            unsafe { extend_lifetime(fut) }
        };

        Self {
            future,
            engine: pinned,
            version,
            ty,
        }
    }

    #[inline]
    pub(crate) fn future(&mut self) -> Pin<&mut dyn Future<Output = EngineResult>> {
        self.future.as_mut()
    }

    #[inline]
    pub(crate) fn engine(&self) -> Pin<&dyn Engine> {
        self.engine.as_ref()
    }

    #[inline]
    pub(crate) fn engine_mut(&mut self) -> Pin<&mut dyn Engine> {
        self.engine.as_mut()
    }

    #[inline]
    pub(crate) fn engine_type(&self) -> EngineType {
        self.ty
    }

    #[inline]
    pub(crate) fn version(&self) -> Version {
        self.version.clone()
    }

    pub(crate) fn handle_request(&mut self, request: Vec<u8>, cred: UCred) -> anyhow::Result<()> {
        self.engine.handle_request(request, cred)
    }

    /// Detach current engine in prepare for upgrade
    /// Some preparatory work is done during this step
    /// e.g., flush inter-engine shared queues
    /// There is no need to call this function if only moves
    pub(crate) fn detach(self) -> Box<dyn Engine> {
        drop(self.future);
        let engine = self.engine;
        unsafe { Pin::into_inner_unchecked(engine) }
    }

    pub(crate) fn flush(&mut self) -> anyhow::Result<()> {
        self.engine.flush()
    }
}
