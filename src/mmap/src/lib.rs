#[allow(clippy::len_without_is_empty)]
pub mod fixed;
pub use fixed::MmapFixed;

#[allow(clippy::len_without_is_empty)]
pub mod aligned;
pub use aligned::MmapAligned;
