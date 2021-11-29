use std::cell::RefCell;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

use log::{info, error};
use spin::Mutex;
use thiserror::Error;

use crate::{Engine, EngineStatus};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid engine ID: {0}, (0 <= expected < {})", num_cpus::get())]
    InvalidId(usize),
    #[error("Fail to set thread affinity")]
    SetAffinity(io::Error),
}

type Result<T> = std::result::Result<T, Error>;

/// __Safety__: A __Runtime__ only have one single consumer which iterates through
/// `running` and runs each engine. Newly added or stolen engines are
/// appended to `pending`, which is protected by a spinlock. In the mainloop,
/// the runtime moves engines from `pending` to `running` by checking the `new_pending` flag.
unsafe impl Sync for Runtime {}

pub struct Runtime {
    /// engine id
    pub(crate) _id: usize,
    // we use RefCell here for unsynchronized interior mutability.
    // Engine has only one consumer, thus, no need to lock it.
    pub(crate) running: RefCell<Vec<RefCell<Box<dyn Engine>>>>,

    pub(crate) new_pending: AtomicBool,
    pub(crate) pending: Mutex<Vec<RefCell<Box<dyn Engine>>>>,
}

impl Runtime {
    pub fn new(id: usize) -> Self {
        Runtime {
            _id: id,
            running: RefCell::new(Vec::new()),
            new_pending: AtomicBool::new(false),
            pending: Mutex::new(Vec::new()),
        }
    }

    /// Returns true if there is no runnable engine or pending engine.
    pub fn is_empty(&self) -> bool {
        self.is_spinning() && self.pending.lock().is_empty()
    }

    /// Returns true if there is no runnable engine.
    pub fn is_spinning(&self) -> bool {
        self.running.borrow().is_empty()
    }

    pub fn add_engine(&self, engine: Box<dyn Engine>) {
        self.pending.lock().push(RefCell::new(engine));
        self.new_pending.store(true, Ordering::Release);
    }

    pub(crate) fn mainloop(&self) -> Result<()> {
        let mut shutdown = Vec::new();
        loop {
            for (index, engine) in self.running.borrow().iter().enumerate() {
                // assert!(!engine.borrow_mut().resume());
                match engine.borrow_mut().resume() {
                    Ok(EngineStatus::NoWork | EngineStatus::Continue) => {
                    }
                    Ok(EngineStatus::Complete) => {
                        // drop engine
                        info!("Engine completed, shutting down.");
                        shutdown.push(index);
                    }
                    Err(e) => {
                        error!("Engine error: {}", e);
                        // drop engine
                    }
                }
            }

            // garbage collect every several rounds, maybe move to another thread.
            for index in shutdown.drain(..).rev() {
                let engine = self.running.borrow_mut().swap_remove(index);
                // make it explicit
                drop(engine);
            }

            // move newly added runtime to the scheduling queue
            if self.new_pending.load(Ordering::Acquire) {
                self.new_pending.store(false, Ordering::Relaxed);
                self.running.borrow_mut().append(&mut self.pending.lock());
            }
        }
    }
}
