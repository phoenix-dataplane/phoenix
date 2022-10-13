use std::cell::RefCell;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use std::thread;
use std::time::Duration;

use minstant::Instant;
use spin::Mutex;
use thiserror::Error;

use super::group::SchedulingGroup;
use super::{EngineContainer, EngineResult};

/// This indicates the runtime of an engine's status.
#[derive(Debug)]
pub(crate) struct Indicator(pub(crate) usize);

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
        Indicator(x)
    }

    #[inline]
    pub(crate) fn set_busy(&mut self) {
        self.0 = Self::BUSY;
    }

    #[inline]
    pub(crate) fn nwork(&self) -> usize {
        self.0
    }

    #[inline]
    pub(crate) fn set_nwork(&mut self, nwork: usize) {
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
    /// Runtime id
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
    // pub(crate) fn add_engine(&self, engine: EngineContainer, dedicated: bool) {
    pub(crate) fn add_engine(&self, scheduling_group: SchedulingGroup, dedicated: bool) {
        // TODO(cjr): FIXME
        // immediate update the dedicate bit
        self.dedicated.fetch_or(dedicated, Ordering::Release);

        log::info!("Runtime {}, adding {:?}", self.id, scheduling_group);

        // adding the engines
        // TODO(cjr): Also record the scheduling_group.id so that when moving engines around runtime,
        // we can know which engines should be moved together.
        let mut pending = self.pending.lock();
        for engine in scheduling_group.engines {
            pending.push(RefCell::new(engine));
        }
        self.new_pending.store(true, Ordering::Release);
    }

    #[inline]
    fn save_energy_or_shutdown(&self, last_event_ts: Instant) {
        // THRES:DURA = 20:1 will lose around 10% bandwidth which is unacceptable,
        // 200:1 looks good so far.

        // goes into sleep mode after 1000 us
        const SLEEP_THRESHOLD: Duration = Duration::from_micros(1000);
        const SLEEP_DURATION: Duration = Duration::from_micros(5);
        // goes into deep sleep after 10 ms
        const DEEP_SLEEP_THRESHOLD: Duration = Duration::from_millis(10);
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

        let mut cnt = 0;
        let mut last_event_ts = Instant::now();
        let mut durs = Vec::with_capacity(128);

        loop {
            // TODO(cjr): if there's no active engine on this runtime, call `mwait` to put the CPU
            // into an optimized state. (the wakeup latency and whether it can be used in user mode
            // are two concerns)
            self.save_energy_or_shutdown(last_event_ts);

            // let mut timer = if self.id == 2 {
            //     Some(crate::timer::Timer::new())
            // } else {
            //     None
            // };

            // let mut has_work = 0;

            // drive each engine
            for (index, engine) in self.running.borrow().iter().enumerate() {
                let mut engine = engine.borrow_mut();

                // Set engine's local storage here before poll
                engine.engine_mut().set_els();

                // bind to a variable first (otherwise engine is borrowed in the match expression)
                cnt += 1;
                let ret = if cnt & 0xffff == 0 {
                    let start = Instant::now();
                    let ret = engine.future().poll(&mut cx);
                    durs.push(start.elapsed());
                    if durs.len() >= 64 {
                        log::warn!("durs: {:?}", durs);
                        unsafe { durs.set_len(0) };
                    }
                    ret
                } else {
                    engine.future().poll(&mut cx)
                };
                match ret {
                    Poll::Pending => {
                        let tracker = engine.engine_mut().tracker();
                        // has_work += tracker.nwork();
                        if tracker.nwork() > 0 {
                            last_event_ts = Instant::now();
                        }
                        tracker.set_nwork(0);
                    }
                    Poll::Ready(EngineResult::Ok(())) => {
                        log::info!(
                            "Engine [{}] completed, shutting down...",
                            engine.engine().description()
                        );
                        shutdown.push(index);
                    }
                    Poll::Ready(EngineResult::Err(e)) => {
                        log::error!("Engine [{}] error: {}", engine.engine().description(), e);
                        shutdown.push(index);
                    }
                }
            }

            // if timer.is_some() {
            //     timer.as_mut().unwrap().tick();
            // }

            // garbage collect every several rounds, maybe move to another thread.
            for index in shutdown.drain(..).rev() {
                let engine = self.running.borrow_mut().swap_remove(index);
                let desc = engine.borrow().engine().description();
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

            // if timer.is_some() {
            //     timer.as_mut().unwrap().tick();
            //     log::debug!("Runtime {}, work: {}, {}", self.id, has_work, timer.unwrap())
            //     // sometimes 300ns, sometimes 1-3us
            // }
        }
    }
}
