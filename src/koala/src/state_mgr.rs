use std::collections::hash_map;
use std::sync::Arc;

use fnv::FnvHashMap as HashMap;

use nix::unistd::Pid;

pub(crate) trait StateTrait: Clone {
    type Err;
    fn new(sm: Arc<StateManager<Self>>, pid: Pid) -> Result<Self, Self::Err>;
}

// Per-user-application-process state
pub(crate) struct StateManager<S> {
    pub(crate) states: spin::Mutex<HashMap<Pid, S>>,
}

impl<S: StateTrait> StateManager<S> {
    pub(crate) fn new() -> Self {
        StateManager {
            states: spin::Mutex::new(HashMap::default()),
        }
    }

    pub(crate) fn get_or_create_state(
        self: &Arc<Self>,
        pid: Pid,
    ) -> Result<S, <S as StateTrait>::Err> {
        let mut states = self.states.lock();
        match states.entry(pid) {
            hash_map::Entry::Occupied(e) => {
                // refcount ++
                Ok(e.get().clone())
            }
            hash_map::Entry::Vacant(e) => {
                let state = S::new(Arc::clone(&self), pid)?;
                // refcount ++
                let ret = state.clone();
                e.insert(state);
                Ok(ret)
            }
        }
    }
}
