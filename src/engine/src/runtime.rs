use std::cell::RefCell;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use spin::Mutex;
use thiserror::Error;

use crate::Engine;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid engine ID: {0}, (0 <= expected < {})", num_cpus::get())]
    InvalidId(usize),
    #[error("Fail to set thread affinity")]
    SetAffinity(#[from] io::Error),
}

type Result<T> = std::result::Result<T, Error>;

/// __Safety__: A __Runtime__ only have one single consumer which iterates through
/// the `running` and runs each engine. Newly added or stolen running are
/// appended to `pending`, which is protected by a spinlock. In the mainloop,
/// the runtime moves engines from `pending` to `runing`.
unsafe impl Sync for Runtime {}

pub struct Runtime {
    /// engine id
    pub(crate) id: usize,
    // we use RefCell here for unsynchronized interior mutability.
    // Engine has only one consumer, thus, no need to lock it.
    pub(crate) running: RefCell<Vec<Box<dyn Engine>>>,

    pub(crate) new_pending: AtomicBool,
    pub(crate) pending: Mutex<Vec<Box<dyn Engine>>>,
}

impl Runtime {
    pub fn new(id: usize) -> Self {
        Runtime {
            id,
            running: RefCell::new(Vec::new()),
            new_pending: AtomicBool::new(false),
            pending: Mutex::new(Vec::new()),
        }
    }

    pub fn add_engine(&self, engine: Box<dyn Engine>) {
        self.pending.lock().push(engine);
        self.new_pending.store(true, Ordering::Release);
    }

    pub(crate) fn mainloop(&self) -> Result<()> {
        loop {
            for engine in self.running.borrow_mut().iter_mut() {
                engine.run();
            }
            // move newly added running to the scheduling queue
            if self.new_pending.load(Ordering::Acquire) {
                self.new_pending.store(false, Ordering::Relaxed);
                self.running.borrow_mut().append(&mut self.pending.lock());
            }
        }
    }
}
