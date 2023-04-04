//! A set of shared-memory related utilities that are intended to be used as replacement
//! for the ones in [`std`], [`alloc`], or [`core`].

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

/// A replacement for [`core::ptr`].
///
/// [`core::ptr`]: https://doc.rust-lang.org/nightly/core/ptr/index.html
#[allow(clippy::not_unsafe_ptr_arg_deref)]
pub mod ptr;

/// A replacement for [`alloc::alloc`].
///
/// [`alloc::alloc`]: https://doc.rust-lang.org/nightly/alloc/alloc/index.html
pub mod alloc;

/// A replacement for [`alloc::boxed`].
///
/// [`alloc::boxed`]: https://doc.rust-lang.org/nightly/alloc/boxed/index.html
#[allow(unused)]
pub mod boxed;

pub(crate) mod raw_vec;

/// A replacement for [`alloc::vec`].
///
/// [`alloc::vec`]: https://doc.rust-lang.org/nightly/alloc/vec/index.html
#[allow(clippy::bool_comparison)]
#[allow(clippy::partialeq_ne_impl)]
#[allow(unused)]
pub mod vec;

/// A replacement for [`alloc::collections`].
///
/// [`alloc::collections`]: https://doc.rust-lang.org/nightly/alloc/collections/index.html
pub mod collections;

/// Shared-memory version of [`std::string::String`].
#[allow(clippy::partialeq_ne_impl)]
pub mod string;
