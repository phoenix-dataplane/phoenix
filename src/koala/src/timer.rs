use std::time::Duration;
use std::fmt;

use minstant::Instant;
use smallvec::SmallVec;

#[derive(Debug, Clone)]
pub(crate) struct Timer {
    start: Instant,
    durations: SmallVec<[Duration; 8]>,
}

impl Timer {
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            start: Instant::now(),
            durations: Default::default(),
        }
    }

    #[inline]
    pub(crate) fn tick(&mut self) {
        self.durations.push(self.start.elapsed());
    }
}

impl Default for Timer {
    fn default() -> Self {
        Timer::new()
    }
}

impl fmt::Display for Timer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Timer")
         .field("duras", &self.durations)
         .finish()
    }
}
