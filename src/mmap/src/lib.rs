#![allow(clippy::len_without_is_empty)]
pub mod fixed;
pub use fixed::MmapFixed;

pub mod aligned;
pub use aligned::MmapAligned;

pub mod mmap;
pub use mmap::{Mmap, MmapOptions};

#[no_mangle]
pub fn test_load_module(a: i32, b: i32) -> i32 {
    eprintln!("test_load_module, cheers!");
    a + b
}
