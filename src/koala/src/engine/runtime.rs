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

use super::group::GroupId;
use super::manager::{EngineId, RuntimeId, RuntimeManager};
use super::{EngineContainer, EngineResult, Indicator, SchedulingGroup};

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

enum RuntimeSubmission {
    NewGroup(SchedulingGroup),
    AttachToGroup(GroupId, Vec<(EngineId, EngineContainer)>),
}

pub(crate) struct Runtime {
    /// runtime id
    pub(crate) _id: RuntimeId,
    // we use RefCell here for unsynchronized interior mutability.
    // Engine has only one consumer, thus, no need to lock it.
    pub(crate) running: RefCell<Vec<RefCell<SchedulingGroup>>>,

    pub(crate) new_pending: AtomicBool,
    pending: Mutex<Vec<RuntimeSubmission>>,

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

    /// Submit a scheduling group to the runtime
    pub(crate) fn add_group(&self, group: SchedulingGroup) {
        let submission = RuntimeSubmission::NewGroup(group);
        self.pending.lock().push(submission);
        self.new_pending.store(true, Ordering::Release);
    }

    /// Attach an engine to a existing scheduling roup
    pub(crate) fn attach_engines_to_group(
        &self,
        gid: GroupId,
        engines: Vec<(EngineId, EngineContainer)>,
    ) {
        let submission = RuntimeSubmission::AttachToGroup(gid, engines);
        self.pending.lock().push(submission);
        self.new_pending.store(true, Ordering::Release);
    }

    /// Submit a request to a specified engine
    pub(crate) fn submit_engine_request(&self, eid: EngineId, request: Vec<u8>, cred: UCred) {
        self.control_requests.lock().push((eid, request, cred));
        self.new_ctrl_request.store(true, Ordering::Release);
    }

    pub(crate) fn request_suspend(&self, eid: EngineId) {
        self.suspend_requests.lock().push(eid);
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
            for (group_index, group) in self.running.borrow().iter().enumerate() {
                let mut guard = group.borrow_mut();
                for (engine_index, (_eid, engine)) in guard.engines.iter_mut().enumerate() {
                    engine.set_els();
                    // bind to a variable first (otherwise engine is borrowed in the match expression)
                    let ret = engine.future().poll(&mut cx);
                    match ret {
                        Poll::Pending => {
                            engine.with_indicator(|indicator| {
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
                                engine.description()
                            );
                            shutdown.push((group_index, engine_index));
                        }
                        Poll::Ready(EngineResult::Err(e)) => {
                            log::error!("Engine [{}] error: {}", engine.description(), e);
                            shutdown.push((group_index, engine_index));
                        }
                    }
                }
            }

            // garbage collect every several rounds, maybe move to another thread.
            for (group_index, engine_index) in shutdown.drain(..).rev() {
                let mut guard = self.running.borrow_mut();
                let (eid, engine) = guard[group_index]
                    .borrow_mut()
                    .engines
                    .swap_remove(engine_index);
                let desc = engine.description().to_owned();
                drop(engine);
                log::info!("Engine [{}] shutdown successfully", desc);
                if guard[group_index].borrow_mut().engines.is_empty() {
                    // All engines in the scheduling group has shutdown
                    guard.swap_remove(group_index);
                }
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
                let mut guard = self.running.borrow_mut();
                for submission in self.pending.lock().drain(..) {
                    match submission {
                        RuntimeSubmission::NewGroup(group) => {
                            guard.push(RefCell::new(group));
                        }
                        RuntimeSubmission::AttachToGroup(group_id, engines) => {
                            match guard.iter_mut().find(|x| x.borrow().id == group_id) {
                                Some(group) => {
                                    group.borrow_mut().engines.extend(engines);
                                }
                                None => {
                                    // handle the case that all engines within the group already shutdowns
                                    let group = SchedulingGroup::new(group_id, engines);
                                    guard.push(RefCell::new(group));
                                }
                            }
                        }
                    }
                }
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
                let mut engines = Vec::with_capacity(engine_ids.len());
                for group in running.iter_mut() {
                    let mut group_guard = group.borrow_mut();
                    let group_engines_suspend = group_guard
                        .engines
                        .drain_filter(|e| engine_ids.contains(&e.0));
                    engines.extend(group_engines_suspend);
                }
                for (engine_id, engine) in engines {
                    tracing::info!(
                        "Engine {:?} revmoed from runtime",
                        engine.description().to_string()
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
                    for group in running.iter_mut() {
                        let mut group_guard = group.borrow_mut();
                        if let Some((_, engine)) = group_guard
                            .engines
                            .iter_mut()
                            .find(|(eid, _)| *eid == target_eid)
                        {
                            if let Err(err) = engine.handle_request(request, cred) {
                                tracing::error!(
                                    "Error in handling engine request, eid={:?}, error: {:?}",
                                    target_eid,
                                    err,
                                );
                            }
                            break;
                        }
                    }
                }
            }
        }
    }
}
