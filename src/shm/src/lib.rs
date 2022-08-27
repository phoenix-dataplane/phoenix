#![feature(negative_impls)]
#![feature(peer_credentials_unix_socket)]
#![feature(allocator_api)]
#![feature(nonnull_slice_from_raw_parts)]
#![feature(min_specialization)]
#![feature(strict_provenance)]
// boxed.rs
// TODO: clean up
#![feature(exact_size_is_empty)]
#![feature(ptr_internals)]
#![feature(ptr_metadata)]
#![feature(core_intrinsics)]
#![feature(ptr_const_cast)]
#![feature(try_reserve_kind)]
#![feature(trusted_len)]
#![feature(extend_one)]
#![feature(rustc_attrs)]
#![feature(slice_ptr_get)]
#![feature(slice_ptr_len)]
// shmptr.rs
#![feature(const_trait_impl)]
#![feature(unix_socket_ancillary_data)]
#![feature(slice_index_methods)]
#![feature(const_ptr_as_ref)]
#![feature(const_mut_refs)]
#![feature(const_ptr_is_null)]
#![feature(const_slice_from_raw_parts)]
#![feature(const_slice_ptr_len)]

/// shm::ptr is similar to core::ptr
pub mod ptr;

/// shm::alloc is similar to alloc::alloc
pub mod alloc;

/// shm::boxed is similar to alloc::boxed
#[allow(unused)]
pub mod boxed;

pub(crate) mod raw_vec;
/// shm::vec is similar to alloc::vec
#[allow(unused)]
pub mod vec;

/// shm::collections is similar to alloc::collections
pub mod collections;
