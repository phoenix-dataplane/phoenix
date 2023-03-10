#![allow(clippy::len_without_is_empty)]
pub mod fixed;
pub use fixed::MmapFixed;

pub mod aligned;
pub use aligned::MmapAligned;

pub mod mmap;
pub use self::mmap::{Mmap, MmapOptions};
