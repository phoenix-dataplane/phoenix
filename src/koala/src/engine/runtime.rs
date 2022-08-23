use std::cell::RefCell;
use std::collections::HashSet;
use std::io;
use std::os::unix::ucred::UCred;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::thread;
use std::time::Duration;

use dashmap::DashMap;
use minstant::Instant;
use spin::Mutex;
use thiserror::Error;

use super::manager::{EngineId, RuntimeId, RuntimeManager};
use super::{EngineContainer, EngineResult, Indicator};

#[allow(unused)]
impl Indicator {
    pub(crate) const BUSY: usize = usize::MAX;

    #[inline]
    pub(crate) fn new(x: usize) -> Self {
        Indicator(Arc::new(AtomicUsize::new(x)))
    }

    #[inline]
    pub fn set_busy(&self) {
        self.0.store(Self::BUSY, Ordering::Relaxed)
    }

    #[inline]
    pub fn set_nwork(&self, nwork: usize) {
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
    #[allow(dead_code)]
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

/// Result for a suspend request
pub(crate) enum SuspendResult {
    /// Container to suspended engine
    Engine(EngineContainer),
    /// Engine not found in current runtime,
    /// either it is already shutdown
    /// or not exists
    NotFound,
}

pub(crate) struct Runtime {
    /// runtime id
    pub(crate) _id: RuntimeId,
    // we use RefCell here for unsynchronized interior mutability.
    // Engine has only one consumer, thus, no need to lock it.
    pub(crate) running: RefCell<Vec<(EngineId, RefCell<EngineContainer>)>>,

    pub(crate) new_pending: AtomicBool,
    pub(crate) pending: Mutex<Vec<(EngineId, RefCell<EngineContainer>)>>,

    pub(crate) new_suspend: AtomicBool,
    pub(crate) suspend_requests: Mutex<Vec<EngineId>>,
    /// Engines that are still active but suspended
    /// They may be moved to another runtime,
    /// or detach and live upgrade to a new version.
    pub(crate) suspended: DashMap<EngineId, SuspendResult>,

    pub(crate) new_ctrl_request: AtomicBool,
    pub(crate) control_requests: Mutex<Vec<(EngineId, Vec<u8>, UCred)>>,

    pub(crate) runtime_manager: Arc<RuntimeManager>,
}

impl Runtime {
    pub(crate) fn new(id: RuntimeId, rm: Arc<RuntimeManager>) -> Self {
        Runtime {
            _id: id,
            running: RefCell::new(Vec::new()),
            new_pending: AtomicBool::new(false),
            pending: Mutex::new(Vec::new()),

            new_suspend: AtomicBool::new(false),
            suspend_requests: Mutex::new(Vec::new()),
            suspended: DashMap::new(),

            new_ctrl_request: AtomicBool::new(false),
            control_requests: Mutex::new(Vec::new()),

            runtime_manager: rm,
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

    pub(crate) fn add_engine(&self, id: EngineId, engine: EngineContainer) {
        self.pending.lock().push((id, RefCell::new(engine)));
        self.new_pending.store(true, Ordering::Release);
    }

    pub(crate) fn submit_request(&self, id: EngineId, request: Vec<u8>, cred: UCred) {
        self.control_requests.lock().push((id, request, cred));
        self.new_ctrl_request.store(true, Ordering::Release);
    }

    pub(crate) fn request_suspend(&self, id: EngineId) {
        self.suspend_requests.lock().push(id);
        self.new_suspend.store(true, Ordering::Release);
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
            thread::park();
        } else if dura > DEEP_SLEEP_THRESHOLD {
            thread::park_timeout(DEEP_SLEEP_DURATION);
        } else if dura > SLEEP_THRESHOLD {
            thread::park_timeout(SLEEP_DURATION);
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
            self.save_energy_or_shutdown(last_event_ts);

            // drive each engine
            for (index, (_eid, engine)) in self.running.borrow().iter().enumerate() {
                engine.borrow_mut().set_els();

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
                let (eid, engine) = self.running.borrow_mut().swap_remove(index);
                let desc = engine.borrow().description().to_owned();
                drop(engine);
                log::info!("Engine [{}] shutdown successfully", desc);
                self.runtime_manager.register_engine_shutdown(eid);
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

            if Ok(true)
                == self.new_suspend.compare_exchange(
                    true,
                    false,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                )
            {
                let mut engine_ids = {
                    let mut guard = self.suspend_requests.lock();
                    guard.drain(..).collect::<HashSet<_>>()
                };

                let mut running = self.running.borrow_mut();
                let engines = running
                    .drain_filter(|(engine_id, _)| engine_ids.contains(engine_id))
                    .collect::<Vec<_>>();
                for (engine_id, engine) in engines {
                    let engine = engine.into_inner();
                    tracing::info!(
                        "Remove engine {:?} from runtime",
                        engine.description().to_owned()
                    );
                    self.suspended
                        .insert(engine_id, SuspendResult::Engine(engine));
                    engine_ids.remove(&engine_id);
                }

                for engine_id in engine_ids {
                    self.suspended.insert(engine_id, SuspendResult::NotFound);
                }
            }

            if Ok(true)
                == self.new_ctrl_request.compare_exchange(
                    true,
                    false,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                )
            {
                let mut guard = self.control_requests.lock();
                for (target_eid, request, cred) in guard.drain(..) {
                    let mut running = self.running.borrow_mut();
                    if let Some((_, container)) =
                        running.iter_mut().find(|(eid, _)| target_eid == *eid)
                    {
                        if let Err(err) = container.borrow_mut().handle_request(request, cred) {
                            tracing::error!(
                                "Error in handling engine request, eid={:?}, error: {:?}",
                                target_eid,
                                err,
                            );
                        }
                    }
                }
            }
        }
    }
}
