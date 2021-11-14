//! Buffer to hold the fat pointer of a slice.
use serde::{Deserialize, Serialize};
use zerocopy::{AsBytes, FromBytes};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, FromBytes, AsBytes)]
pub struct Buffer {
    pub addr: u64,
    pub len: u64,
}

impl<T> From<&[T]> for Buffer {
    fn from(s: &[T]) -> Self {
        let r = s.as_ptr_range();
        Buffer {
            addr: r.start as u64,
            len: r.end as u64 - r.start as u64,
        }
    }
}

impl<T> From<&mut [T]> for Buffer {
    fn from(s: &mut [T]) -> Self {
        let r = s.as_ptr_range();
        Buffer {
            addr: r.start as u64,
            len: r.end as u64 - r.start as u64,
        }
    }
}
