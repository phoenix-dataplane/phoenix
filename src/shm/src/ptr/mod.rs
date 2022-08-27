//! Shared memory pointer types.

pub mod shm_non_null;
pub use shm_non_null::ShmNonNull;

pub mod shmptr;
pub use shmptr::ShmPtr;
