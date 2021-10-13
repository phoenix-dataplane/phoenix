//! Runtime manager is the control plane of runtimes. It is responsible for
//! creating/destructing runtimes, map runtimes to cores, balance the work
//! among different runtimes, and even dynamically scale out/down the runtimes.
use std::io;
use std::thread::{self, JoinHandle};
// use crossbeam::thread::{self, ScopedJoinHandle};

use crate::runtime::{self, Runtime};
use crate::Engine;

pub struct RuntimeManager {
    runtimes: Vec<Runtime>,
    handles: Vec<JoinHandle<Result<(), runtime::Error>>>,
}

impl RuntimeManager {
    pub fn new(n: usize) -> Self {
        RuntimeManager {
            runtimes: Vec::with_capacity(n),
            handles: Vec::with_capacity(n),
        }
    }

    pub fn start_runtime(&self, core: usize) {
        let runtime = Runtime::new(core);
        self.runtimes.push(runtime);
        let handle = thread::spawn(|| {
            // check engine id
            let num_cpus = num_cpus::get();
            if core >= num_cpus {
                return Err(runtime::Error::InvalidId(core));
            }
            // set thread affinity, may dedicate this task to a load balancer.
            scheduler::set_self_affinity(scheduler::CpuSet::single(core))
                .map_err(|_| runtime::Error::SetAffinity(io::Error::last_os_error()))?;
            self.runtimes.last().as_mut().unwrap().mainloop()
        });

        self.handles.push(handle);
    }

    fn submit(&self, engine: Box<dyn Engine>) {
        // always submit to the first runtime
        if self.runtimes.is_empty() {
            self.start_runtime(0);
        }
        self.runtimes[0].add_engine(engine)
    }
}