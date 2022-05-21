use std::cell::RefCell;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use spin::Mutex;
use thiserror::Error;

use super::{EngineContainer, EngineLocalStorage, EngineResult};

thread_local! {
    /// To emulate a thread local storage (TLS). This should be called engine-local-storage (ELS).
    pub(crate) static ENGINE_LS: RefCell<Option<&'static dyn EngineLocalStorage>> = RefCell::new(None);
}

/// This indicates the runtime of an engine's status.
#[derive(Debug)]
pub(crate) struct Indicator(pub(crate) Arc<AtomicUsize>);

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

impl Indicator {
    pub(crate) const BUSY: usize = usize::MAX;

    #[inline]
    pub(crate) fn new(x: usize) -> Self {
        Indicator(Arc::new(AtomicUsize::new(x)))
    }

    #[inline]
    pub(crate) fn set_busy(&self) {
        self.0.store(Self::BUSY, Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn set_nwork(&self, nwork: usize) {
        // TODO(cjr): double-check if this is OK
        self.0.store(nwork, Ordering::Relaxed)
    }

    #[inline]
    pub(crate) fn is_busy(&self) -> bool {
        self.0.load(Ordering::Relaxed) == Self::BUSY
    }

    #[inline]
    pub(crate) fn is_spinning(&self) -> bool {
        self.0.load(Ordering::Relaxed) == 0
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid engine ID: {0}, (0 <= expected < {})", num_cpus::get())]
    InvalidId(usize),
    #[allow(dead_code)]
    #[error("Fail to set thread affinity")]
    SetAffinity(io::Error),
}

/// # Safety
///
/// A __Runtime__ only have one single consumer which iterates through
/// `running` and runs each engine. Newly added or stolen engines are
/// appended to `pending`, which is protected by a spinlock. In the mainloop,
/// the runtime moves engines from `pending` to `running` by checking the `new_pending` flag.
unsafe impl Sync for Runtime {}

pub(crate) struct Runtime {
    /// engine id
    pub(crate) _id: usize,
    // we use RefCell here for unsynchronized interior mutability.
    // Engine has only one consumer, thus, no need to lock it.
    pub(crate) running: RefCell<Vec<RefCell<EngineContainer>>>,

    pub(crate) new_pending: AtomicBool,
    pub(crate) pending: Mutex<Vec<RefCell<EngineContainer>>>,
}

impl Runtime {
    pub(crate) fn new(id: usize) -> Self {
        Runtime {
            _id: id,
            running: RefCell::new(Vec::new()),
            new_pending: AtomicBool::new(false),
            pending: Mutex::new(Vec::new()),
        }
    }

    /// Returns true if there is no runnable engine or pending engine.
    pub(crate) fn is_empty(&self) -> bool {
        self.is_spinning() && self.pending.lock().is_empty()
    }

    /// Returns true if there is no runnable engine.
    pub(crate) fn is_spinning(&self) -> bool {
        self.running.borrow().is_empty()
    }

    pub(crate) fn add_engine(&self, engine: EngineContainer) {
        self.pending.lock().push(RefCell::new(engine));
        self.new_pending.store(true, Ordering::Release);
    }

    /// A spinning future executor.
    pub(crate) fn mainloop(&self) -> std::result::Result<(), Error> {
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);

        let mut shutdown = Vec::new();

        loop {
            // TODO(cjr): if there's no active engine on this runtime, call `mwait` to put the CPU
            // into an optimized state. (the wakeup latency and whether it can be used in user mode
            // are two concerns)

            // drive each engine
            for (index, engine) in self.running.borrow().iter().enumerate() {
                // TODO(cjr): Set engine's local storage here before poll
                ENGINE_LS.with(|els| {
                    // Safety: The purpose here is to emulate thread-local storage for Engine.
                    // Different engines can expose different types of TLS. The TLS will only be
                    // used during the runtime of an engine. Thus, the TLS should always be valid
                    // when it is referred to. However, there is no known way (for me) to express
                    // this lifetime constraint. Ultimately, the solution is to force the lifetime
                    // of the TLS of an engine to be 'static, and the programmer needs to think
                    // through to make sure they implement it correctly.
                    *els.borrow_mut() = unsafe { engine.borrow().els() };
                });

                // bind to a variable first (otherwise engine is borrowed in the match expression)
                let ret = engine.borrow_mut().future().poll(&mut cx);
                match ret {
                    Poll::Pending => {
                        // where do I get the work you have done?
                    }
                    Poll::Ready(EngineResult::Ok(())) => {
                        info!(
                            "Engine [{}] completed, shutting down...",
                            engine.borrow().description()
                        );
                        shutdown.push(index);
                    }
                    Poll::Ready(EngineResult::Err(e)) => {
                        error!("Engine [{}] error: {}", engine.borrow().description(), e);
                        shutdown.push(index);
                    }
                }
            }

            // garbage collect every several rounds, maybe move to another thread.
            for index in shutdown.drain(..).rev() {
                let engine = self.running.borrow_mut().swap_remove(index);
                let desc = engine.borrow().description().to_owned();
                drop(engine);
                info!("Engine [{}] shutdown successfully", desc);
            }

            // move newly added runtime to the scheduling queue
            if self.new_pending.load(Ordering::Acquire) {
                self.new_pending.store(false, Ordering::Relaxed);
                self.running.borrow_mut().append(&mut self.pending.lock());
            }
        }
    }
}
