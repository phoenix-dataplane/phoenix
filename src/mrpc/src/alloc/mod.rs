#[allow(unused)]
mod boxed;

#[allow(unused)]
mod raw_vec;

#[allow(unused)]
mod vec;
// SAFETY: It is the caller's responsibility to ensure all alloc's types are created on sender heap
// otherwise a ManuallyDrop wrapper is required
// to prevent improper deallocation
// as in `Drop` impl, we consider memory is allocated on sender heap.
pub(crate) use boxed::Box;

pub use vec::Vec;
