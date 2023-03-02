#![feature(allocator_api)]
#![feature(strict_provenance)]

pub mod wheap;
pub use wheap::SharedHeapAllocator;

pub mod backend;
pub(crate) mod gc;
