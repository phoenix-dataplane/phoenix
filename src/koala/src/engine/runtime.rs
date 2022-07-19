use std::cell::RefCell;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::thread;
use std::time::Duration;

use minstant::Instant;
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

#[allow(unused)]
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
    pub(crate) id: usize,
    // we use RefCell here for unsynchronized interior mutability.
    // Engine has only one consumer, thus, no need to lock it.
    pub(crate) running: RefCell<Vec<RefCell<EngineContainer>>>,

    // Whether the engine is dedicated or shared.
    pub(crate) dedicated: AtomicBool,

    pub(crate) new_pending: AtomicBool,
    pub(crate) pending: Mutex<Vec<RefCell<EngineContainer>>>,
}

impl Runtime {
    pub(crate) fn new(id: usize) -> Self {
        Runtime {
            id,
            running: RefCell::new(Vec::new()),
            dedicated: AtomicBool::new(false),
            new_pending: AtomicBool::new(false),
            pending: Mutex::new(Vec::new()),
        }
    }

    /// Returns true if there is no runnable engine or pending engine.
    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.is_spinning() && self.pending.lock().is_empty()
    }

    /// Returns true if there is no runnable engine.
    #[inline]
    pub(crate) fn is_spinning(&self) -> bool {
        self.running.try_borrow().map_or(false, |r| r.is_empty())
    }

    #[inline]
    pub(crate) fn is_dedicated(&self) -> bool {
        self.dedicated.load(Ordering::Acquire)
    }

    #[inline]
    pub(crate) fn add_engine(&self, engine: EngineContainer, dedicated: bool) {
        self.dedicated.fetch_or(dedicated, Ordering::Release);
        self.pending.lock().push(RefCell::new(engine));
        self.new_pending.store(true, Ordering::Release);
    }

    #[inline]
    fn save_energy_or_shutdown(&self, last_event_ts: Instant) {
        // goes into sleep mode after 100 us
        const SLEEP_THRESHOLD: Duration = Duration::from_micros(100);
        const SLEEP_DURATION: Duration = Duration::from_micros(5);
        // goes into deep sleep after 1 ms
        const DEEP_SLEEP_THRESHOLD: Duration = Duration::from_millis(1);
        const DEEP_SLEEP_DURATION: Duration = Duration::from_micros(50);
        // shutdown after idle for 1 second
        const SHUTDOWN_THRESHOLD: Duration = Duration::from_secs(1);

        let dura = Instant::now() - last_event_ts;

        // park the engine only then it's empty
        if dura > SHUTDOWN_THRESHOLD && self.is_empty() {
            tracing::trace!("Runtime {} is shutting down", self.id);
            thread::park();
            tracing::trace!("Runtime {} is restarted", self.id);
        } else if dura > DEEP_SLEEP_THRESHOLD {
            tracing::trace!("Runtime {} is going to deep sleep", self.id);
            thread::park_timeout(DEEP_SLEEP_DURATION);
            tracing::trace!("Runtime {} has waked from deep sleep", self.id);
        } else if dura > SLEEP_THRESHOLD {
            tracing::trace!("Runtime {} is going to sleep", self.id);
            thread::park_timeout(SLEEP_DURATION);
            tracing::trace!("Runtime {} has waked from sleep", self.id);
        }
    }

    /// A spinning future executor.
    pub(crate) fn mainloop(&self) -> Result<(), Error> {
        let waker = futures::task::noop_waker();
        let mut cx = Context::from_waker(&waker);

        let mut shutdown = Vec::new();

        // TODO(cjr): use jiffy
        let mut last_event_ts = Instant::now();

        loop {
            // TODO(cjr): if there's no active engine on this runtime, call `mwait` to put the CPU
            // into an optimized state. (the wakeup latency and whether it can be used in user mode
            // are two concerns)
            // self.save_energy_or_shutdown(last_event_ts);

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
                        engine.borrow().with_indicator(|indicator| {
                            // whatever the number reads here does not affect correctness
                            let n = indicator.0.load(Ordering::Relaxed);
                            if n > 0 {
                                last_event_ts = Instant::now();
                            }
                        });
                    }
                    Poll::Ready(EngineResult::Ok(())) => {
                        log::info!(
                            "Engine [{}] completed, shutting down...",
                            engine.borrow().description()
                        );
                        shutdown.push(index);
                    }
                    Poll::Ready(EngineResult::Err(e)) => {
                        log::error!("Engine [{}] error: {}", engine.borrow().description(), e);
                        shutdown.push(index);
                    }
                }
            }

            // garbage collect every several rounds, maybe move to another thread.
            for index in shutdown.drain(..).rev() {
                let engine = self.running.borrow_mut().swap_remove(index);
                let desc = engine.borrow().description().to_owned();
                drop(engine);
                log::info!("Engine [{}] shutdown successfully", desc);
            }

            if self.is_empty() {
                self.dedicated.store(false, Ordering::Release);
            }

            // move newly added runtime to the scheduling queue
            if Ok(true)
                == self.new_pending.compare_exchange(
                    true,
                    false,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                )
            {
                self.running.borrow_mut().append(&mut self.pending.lock());
            }
        }
    }
}
