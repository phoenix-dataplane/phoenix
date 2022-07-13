use std::collections::hash_map;
use std::sync::{Weak, Arc};

use fnv::FnvHashMap as HashMap;
use nix::unistd::Pid;

pub trait ProcessShared {
    type Err;

    fn new(pid: Pid) -> Result<Self, Self::Err>;
}

/// Per-user-application-process shared state
pub struct SharedStateManager<S> {
    // TODO(wyj): determine whether we should wrap states in Mutex?
    // i.e., restoring engines in dedicated thread or on main control thread? 
    pub states: spin::Mutex<HashMap<Pid, Weak<S>>>,
}

impl<S: ProcessShared> SharedStateManager<S> {
    pub fn new() -> Self {
        SharedStateManager {
            states: HashMap::new()
        }
    }

    pub fn get_or_create(&self, pid: Pid) -> Result<Arc<S>, <S as ProcessShared>::Err>  {
        let states = self.states.lock();
        match self.states.entry(pid) {
            hash_map::Entry::Occupied(entry) => {
                if let Some(state) = entry.get().upgrade() {
                    Ok(state)
                }
                else {
                    let state = S::new(pid)?;
                    let wrapped = Arc::new(state);
                    entry.insert(Arc::downgrade(&wrapped));
                    Ok(wrapped)
                }
            }
            hash_map::Entry::Vacant(entry) => {
                let state = S::new(pid)?;
                let wrapped = Arc::new(state);
                entry.insert(Arc::downgrade(&wrapped));
                Ok(wrapped)
            }
        }
    }
}
