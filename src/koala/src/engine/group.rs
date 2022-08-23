//! A group of engine that will be packaged together and always
//! share the same scheduling policy.
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

use interface::engine::SchedulingMode;

use super::container::EngineContainer;

static GROUP_ID_CNT: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct SchedulingGroup {
    /// Group ID.
    #[allow(unused)]
    pub(crate) id: usize,
    /// Scheduling mode
    pub(crate) mode: SchedulingMode,
    /// The engines in this group.
    pub(crate) engines: Vec<EngineContainer>,
}

impl SchedulingGroup {
    /// Construct an empty group with scheduling mode set to `mode`.
    pub(crate) fn empty(mode: SchedulingMode) -> Self {
        SchedulingGroup {
            id: GROUP_ID_CNT.fetch_add(1, Ordering::AcqRel),
            mode,
            engines: Vec::new(),
        }
    }

    /// Construct an SchedulingGroup with a single element and set the scheduling mode set to `mode`.
    pub(crate) fn singleton(mode: SchedulingMode, engine: EngineContainer) -> Self {
        SchedulingGroup {
            id: GROUP_ID_CNT.fetch_add(1, Ordering::AcqRel),
            mode,
            engines: vec![engine],
        }
    }

    pub(crate) fn add(&mut self, engine: EngineContainer) {
        self.engines.push(engine);
    }
}

impl fmt::Debug for SchedulingGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let engine_names: Vec<String> = self
            .engines
            .iter()
            .map(|e| e.engine().description())
            .collect();
        f.debug_struct("SchedulingGroup")
            .field("mode", &self.mode)
            .field("engines", &engine_names)
            .finish()
    }
}
