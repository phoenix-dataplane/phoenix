//! Runtime manager is the control plane of runtimes. It is responsible for
//! creating/destructing runtimes, map runtimes to cores, balance the work
//! among different runtimes, and even dynamically scale out/down the runtimes.
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use spin::Mutex;

use crate::runtime::{self, Runtime};
use crate::Engine;
use crate::SchedulingMode;

pub struct RuntimeManager {
    inner: Mutex<Inner>,
}

struct Inner {
    next_core: usize,
    runtimes: Vec<Arc<Runtime>>,
    handles: Vec<JoinHandle<Result<(), runtime::Error>>>,
}

impl Inner {
    fn schedule_dedicate(&mut self, engine: Box<dyn Engine>) {
        // find a spare runtime
        let rid = match self
            .runtimes
            .iter()
            .enumerate()
            .find(|(_i, r)| r.is_empty())
        {
            Some((rid, _runtime)) => rid,
            None => {
                // if there's no spare runtime, and there are available resources (e.g. cpus),
                // spawn a new one.

                // find the next available CPU and start a runtime on it.
                let rid = self.next_core;
                self.start_runtime(self.next_core);
                self.next_core += 1;
                rid
            }
        };
        self.runtimes[rid].add_engine(engine);
    }
}

impl RuntimeManager {
    pub fn new(n: usize) -> Self {
        let inner = Inner {
            next_core: 0,
            runtimes: Vec::with_capacity(n),
            handles: Vec::with_capacity(n),
        };
        RuntimeManager {
            inner: Mutex::new(inner),
        }
    }

    pub fn submit(&self, engine: Box<dyn Engine>, mode: SchedulingMode) {
        let mut inner = self.inner.lock();
        match mode {
            SchedulingMode::Dedicate => {
                inner.schedule_dedicate(engine);
            }
            SchedulingMode::Compact => unimplemented!(),
            SchedulingMode::Spread => unimplemented!(),
        }
    }
}

impl Inner {
    fn start_runtime(&mut self, core: usize) {
        let runtime = Arc::new(Runtime::new(core));
        self.runtimes.push(runtime);

        let runtime = Arc::clone(self.runtimes.last().unwrap());
        let handle = thread::spawn(move || {
            // check core id
            let num_cpus = num_cpus::get();
            if core >= num_cpus {
                return Err(runtime::Error::InvalidId(core));
            }
            // NOTE(cjr): do not set affinity here. It only hurts the performance if the user app
            // does not run on the hyperthread core pair. Since we cannot expect that we always
            // have hyperthread core pair available, not setting affinity turns out to be better.
            // scheduler::set_self_affinity(scheduler::CpuSet::single(core))
            //     .map_err(|_| runtime::Error::SetAffinity(io::Error::last_os_error()))?;
            runtime.mainloop()
        });

        self.handles.push(handle);
    }
}
