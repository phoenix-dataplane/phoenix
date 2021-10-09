// use std::thread::{self, JoinHandle};

use crossbeam::thread::{self, ScopedJoinHandle};
use spin::Mutex;

use crate::Engine;
// use utils::set_affinity_for_current;

pub struct EngineRuntime {
    id: usize,
    engines: Vec<Box<dyn Engine>>,
    /// work stealing
    pending_engines: Mutex<Vec<Box<dyn Engine>>>,
}

impl EngineRuntime {
    fn new(id: usize) -> Self {
        EngineRuntime {
            id,
            engines: Vec::new(),
            pending_engines: Mutex::new(Vec::new()),
        }
    }

    fn add_engine(&mut self, engine: Box<dyn Engine>) {
        self.pending_engines.lock().push(engine);
    }

    fn start(&mut self) {
        // let handle = thread::spawn(move || {
        //     set_affinity_for_current(self.id);
        //     self.mainloop();
        // });
        // let handle = thread::scope(|s| {
        //     let handle = s.spawn(|_| {
        //     });

        //     handle.join().unwrap();
        // }).unwrap();
        // handle
    }

    fn mainloop(&mut self) {
        loop {
            for engine in &mut self.engines {
                engine.run();
            }
            // steal work from other runtimes
            self.engines.append(&mut self.pending_engines.lock());
        }
    }
}
