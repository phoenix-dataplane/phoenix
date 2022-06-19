//! Buffer to hold the fat pointer of a slice.
use std::slice::SliceIndex;

use serde::{Deserialize, Serialize};
use zerocopy::{AsBytes, FromBytes};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, FromBytes, AsBytes)]
pub struct Range {
    pub offset: u64,
    pub len: u64,
}

impl Range {
    #[inline]
    pub fn new<T, R>(mr: &[T], range: R) -> Self
    where
        R: SliceIndex<[T], Output = [T]>,
    {
        let buffer = range.index(mr);
        let r1 = mr.as_ptr_range();
        let r2 = buffer.as_ptr_range();
        Range {
            offset: (r2.start as u64 - r1.start as u64),
            len: (r2.end as u64 - r2.start as u64),
        }
    }
}
