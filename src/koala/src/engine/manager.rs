//! Runtime manager is the control plane of runtimes. It is responsible for
//! creating/destructing runtimes, map runtimes to cores, balance the work
//! among different runtimes, and even dynamically scale out/down the runtimes.
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use spin::Mutex;

use interface::engine::SchedulingMode;

use super::container::EngineContainer;
use super::runtime::{self, Runtime};
use crate::config::Config;

pub struct RuntimeManager {
    inner: Mutex<Inner>,
}

struct Inner {
    next_core: usize,
    runtimes: Vec<Arc<Runtime>>,
    handles: Vec<JoinHandle<Result<(), runtime::Error>>>,
}

impl Inner {
    fn schedule_dedicate(&mut self, engine: EngineContainer) {
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

        // a runtime will not be parked when having pending engines, so in theory, we can check
        // whether the runtime and only unpark it when it's in parked state.
        self.handles[rid].thread().unpark();
    }
}

impl RuntimeManager {
    pub fn new(_config: &Config) -> Self {
        let inner = Inner {
            next_core: 0,
            runtimes: Vec::with_capacity(1),
            handles: Vec::with_capacity(1),
        };
        RuntimeManager {
            inner: Mutex::new(inner),
        }
    }

    pub(crate) fn submit(&self, engine: EngineContainer, mode: SchedulingMode) {
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
        self.runtimes.push(Arc::clone(&runtime));

        let handle = thread::Builder::new()
            .name(format!("Runtime {}", core))
            .spawn(move || {
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
            })
            .unwrap_or_else(|e| panic!("failed to spawn new threads: {}", e));

        self.handles.push(handle);
    }
}
