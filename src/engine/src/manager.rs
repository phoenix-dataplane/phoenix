//! Runtime manager is the control plane of runtimes. It is responsible for
//! creating/destructing runtimes, map runtimes to cores, balance the work
//! among different runtimes, and even dynamically scale out/down the runtimes.
use std::io;
use std::thread::{self, JoinHandle};
use std::sync::Arc;

use spin::Mutex;

use crate::runtime::{self, Runtime};
use crate::Engine;

pub struct RuntimeManager {
    inner: Mutex<Inner>,
}

struct Inner {
    runtimes: Vec<Arc<Runtime>>,
    handles: Vec<JoinHandle<Result<(), runtime::Error>>>,
}

impl RuntimeManager {
    pub fn new(n: usize) -> Self {
        let inner = Inner {
            runtimes: Vec::with_capacity(n),
            handles: Vec::with_capacity(n),
        };
        RuntimeManager {
            inner: Mutex::new(inner),
        }
    }

    pub fn submit(&self, engine: Box<dyn Engine>) {
        // always submit to the first runtime
        let mut inner = self.inner.lock();
        if inner.runtimes.is_empty() {
            inner.start_runtime(0);
        }
        inner.runtimes[0].add_engine(engine)
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
            // set thread affinity, may dedicate this task to a load balancer.
            scheduler::set_self_affinity(scheduler::CpuSet::single(core))
                .map_err(|_| runtime::Error::SetAffinity(io::Error::last_os_error()))?;
            runtime.mainloop()
        });

        self.handles.push(handle);
    }
}