// boxed.rs & shmptr.rs
#![feature(strict_provenance)]
#![feature(allocator_api)]
#![feature(negative_impls)]
#![feature(peer_credentials_unix_socket)]
#![feature(nonnull_slice_from_raw_parts)]
#![feature(min_specialization)]
#![feature(exact_size_is_empty)]
#![feature(ptr_internals)]
#![feature(ptr_metadata)]
#![feature(core_intrinsics)]
#![feature(try_reserve_kind)]
#![feature(trusted_len)]
#![feature(extend_one)]
#![feature(rustc_attrs)]
#![feature(slice_ptr_get)]
#![feature(slice_ptr_len)]
#![feature(const_trait_impl)]
#![feature(unix_socket_ancillary_data)]
#![feature(slice_index_methods)]
#![feature(const_ptr_as_ref)]
#![feature(const_mut_refs)]
#![feature(const_ptr_is_null)]
#![feature(const_slice_from_raw_parts_mut)]
#![feature(const_slice_ptr_len)]
// string.rs
#![feature(str_internals)]
#![feature(pattern)]
#![feature(slice_range)]
#![feature(utf8_chunks)]

#![allow(clippy::explicit_auto_deref)]
#![allow(clippy::missing_safety_doc)]

/// shm::ptr is similar to core::ptr
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub mod ptr;

/// shm::alloc is similar to alloc::alloc
pub mod alloc;

/// shm::boxed is similar to alloc::boxed
#[allow(unused)]
pub mod boxed;

pub(crate) mod raw_vec;
/// shm::vec is similar to alloc::vec
#[allow(clippy::bool_comparison)]
#[allow(clippy::partialeq_ne_impl)]
#[allow(unused)]
pub mod vec;

/// shm::collections is similar to alloc::collections
pub mod collections;

/// shared-memory version for std::String
#[allow(clippy::partialeq_ne_impl)]
pub mod string;
