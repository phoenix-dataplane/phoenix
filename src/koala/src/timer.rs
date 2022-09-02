use std::fmt;
use std::time::Duration;

use minstant::Instant;
use smallvec::SmallVec;

#[derive(Debug, Clone)]
pub struct Timer {
    start: Instant,
    durations: SmallVec<[Duration; 8]>,
}

impl Timer {
    #[inline]
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            durations: Default::default(),
        }
    }

    #[inline]
    pub fn tick(&mut self) {
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
