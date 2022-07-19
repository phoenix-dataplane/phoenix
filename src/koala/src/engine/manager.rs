//! Runtime manager is the control plane of runtimes. It is responsible for
//! creating/destructing runtimes, map runtimes to cores, balance the work
//! among different runtimes, and even dynamically scale out/down the runtimes.
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::{self, JoinHandle};

use dashmap::DashMap;
use interface::engine::SchedulingMode;
use nix::unistd::Pid;

use super::container::EngineContainer;
use super::runtime::{self, Runtime};
use super::EngineType;
use crate::config::Config;

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EngineId(u64);

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuntimeId(u64);

pub struct RuntimeManager {
    pub inner: Mutex<Inner>,
    // TODO(wyj): handle multiple threads
    pub clients: DashMap<Pid, DashMap<EngineId, (EngineType, RuntimeId)>>,
    // map engines to client Pid
    // TODO(wyj): handle multiple threads
    engines: DashMap<EngineId, Pid>,
}

pub struct Inner {
    engine_counter: u64,
    runtime_counter: u64,
    pub(crate) runtimes: HashMap<RuntimeId, Arc<Runtime>>,
    handles: HashMap<RuntimeId, JoinHandle<Result<(), runtime::Error>>>,
}

impl Inner {
    fn schedule_dedicate(
        &mut self,
        engine: EngineContainer,
        rm: &Arc<RuntimeManager>,
    ) -> (EngineId, RuntimeId) {
        // find a spare runtime
        // NOTE(wyj): iterating over HashMap should be fine
        // as submitting a new engine is not on fast path
        // Moreover, there are only limited number of runtimes
        let rid = match self.runtimes.iter().find(|(_i, r)| r.is_empty()) {
            Some((rid, _runtime)) => *rid,
            None => self.start_runtime(self.runtime_counter as usize, Arc::clone(rm)),
        };

        let engine_id = EngineId(self.engine_counter);
        self.engine_counter += 1;
        self.runtimes[&rid].add_engine(engine_id, engine);

        // a runtime will not be parked when having pending engines, so in theory, we can check
        // whether the runtime and only unpark it when it's in parked state.
        self.handles[&rid].thread().unpark();

        (engine_id, rid)
    }
}

impl RuntimeManager {
    pub fn new(_config: &Config) -> Self {
        let inner = Inner {
            engine_counter: 0,
            runtime_counter: 0,
            runtimes: HashMap::with_capacity(1),
            handles: HashMap::with_capacity(1),
        };
        RuntimeManager {
            inner: Mutex::new(inner),
            clients: DashMap::new(),
            engines: DashMap::new(),
        }
    }

    pub(crate) fn submit(
        self: &Arc<Self>,
        pid: Pid,
        engine: EngineContainer,
        mode: SchedulingMode,
    ) {
        let mut inner = self.inner.lock().unwrap();
        match mode {
            SchedulingMode::Dedicate => {
                let (eid, rid) = inner.schedule_dedicate(engine, self);
                // TODO: write it correct
                let entry = self.clients.entry(pid).or_insert_with(DashMap::new);
                entry.insert(eid, (EngineType("Salloc".to_string()), rid));
                self.engines.insert(eid, pid);
            }
            SchedulingMode::Compact => unimplemented!(),
            SchedulingMode::Spread => unimplemented!(),
        }
    }

    pub(crate) fn register_shutdown(&self, engine_id: EngineId) {
        let (_, pid) = self.engines.remove(&engine_id).unwrap();
        let client = self.clients.get_mut(&pid).unwrap();
        client.remove(&engine_id);
    }
}

impl Inner {
    fn start_runtime(&mut self, core: usize, rm: Arc<RuntimeManager>) -> RuntimeId {
        let runtime_id = RuntimeId(self.runtime_counter);
        self.runtime_counter += 1;
        let runtime = Arc::new(Runtime::new(runtime_id, rm));
        self.runtimes.insert(runtime_id, Arc::clone(&runtime));

        let handle = thread::Builder::new()
            .name(format!("Runtime {}", runtime_id.0))
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

        self.handles.insert(runtime_id, handle);
        runtime_id
    }
}
