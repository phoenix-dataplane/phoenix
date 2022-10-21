use std::collections::hash_map;
use std::sync::{Arc, Weak};

use fnv::FnvHashMap as HashMap;
use nix::unistd::Pid;

pub trait ProcessShared: Sized {
    type Err;

    fn new(pid: Pid) -> Result<Self, Self::Err>;
}

/// Per-user-application-process shared state
pub struct SharedStateManager<S> {
    states: HashMap<Pid, Weak<S>>,
}

impl<S: ProcessShared> Default for SharedStateManager<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: ProcessShared> SharedStateManager<S> {
    pub fn new() -> Self {
        SharedStateManager {
            states: HashMap::default(),
        }
    }

    pub fn get_or_create(&mut self, pid: Pid) -> Result<Arc<S>, <S as ProcessShared>::Err> {
        match self.states.entry(pid) {
            hash_map::Entry::Occupied(mut entry) => {
                if let Some(state) = entry.get().upgrade() {
                    Ok(state)
                } else {
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

    pub fn get_or_create_with<F: FnOnce() -> S>(
        &mut self,
        pid: Pid,
        init: F,
    ) -> Result<Arc<S>, <S as ProcessShared>::Err> {
        match self.states.entry(pid) {
            hash_map::Entry::Occupied(mut entry) => {
                if let Some(state) = entry.get().upgrade() {
                    Ok(state)
                } else {
                    let state = init();
                    let wrapped = Arc::new(state);
                    entry.insert(Arc::downgrade(&wrapped));
                    Ok(wrapped)
                }
            }
            hash_map::Entry::Vacant(entry) => {
                let state = init();
                let wrapped = Arc::new(state);
                entry.insert(Arc::downgrade(&wrapped));
                Ok(wrapped)
            }
        }
    }

    #[inline]
    pub fn contains(&self, pid: Pid) -> bool {
        if let Some(state) = self.states.get(&pid) {
            state.strong_count() > 0
        } else {
            false
        }
    }
}
