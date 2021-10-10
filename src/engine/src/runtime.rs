use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

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

pub struct Runtime {
    /// engine id
    id: usize,
    engines: Vec<Box<dyn Engine>>,

    pending: AtomicBool,
    pending_engines: Mutex<Vec<Box<dyn Engine>>>,
}

impl Runtime {
    pub fn new(id: usize) -> Self {
        Runtime {
            id,
            engines: Vec::new(),
            pending: AtomicBool::new(false),
            pending_engines: Mutex::new(Vec::new()),
        }
    }

    pub fn start(mut self) -> JoinHandle<Result<()>> {
        let handle = thread::spawn(move || {
            // check engine id
            let num_cpus = num_cpus::get();
            if self.id >= num_cpus {
                return Err(Error::InvalidId(self.id));
            }
            // set thread affinity, may dedicate this task to a load balancer.
            scheduler::set_self_affinity(scheduler::CpuSet::single(self.id))
                .map_err(|_| Error::SetAffinity(io::Error::last_os_error()))?;
            self.mainloop()
        });
        handle
    }

    pub fn add_engine(&self, engine: Box<dyn Engine>) {
        self.pending_engines.lock().push(engine);
        self.pending.store(true, Ordering::Release);
    }

    fn mainloop(&mut self) -> Result<()> {
        loop {
            for engine in &mut self.engines {
                engine.run();
            }
            // move newly added engines to the scheduling queue
            if self.pending.load(Ordering::Acquire) {
                self.engines.append(&mut self.pending_engines.lock());
            }
        }
    }
}
