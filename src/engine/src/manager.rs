//! Runtime manager is the control plane of runtimes. It is responsible for
//! creating/destructing runtimes, map runtimes to cores, balance the work
//! among different runtimes, and even dynamically scale out/down the runtimes.

pub struct RuntimeManager {
    runtimes: Vec<Runtime>,
}

impl RuntimeManager {
    pub fn new(n: usize) -> Self {
        RuntimeManager {
            runtimes: Vec::with_capacity(n),
        }
    }
}