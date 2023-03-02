use std::os::unix::io::RawFd;

use serde::{Deserialize, Serialize};

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Handle(pub u64);

impl Handle {
    pub const INVALID: Handle = Handle(u64::MAX);
}

pub trait AsHandle {
    #[must_use]
    fn as_handle(&self) -> Handle;
}

impl AsHandle for RawFd {
    #[inline]
    fn as_handle(&self) -> Handle {
        if *self >= 0 {
            Handle(*self as _)
        } else {
            Handle::INVALID
        }
    }
}
