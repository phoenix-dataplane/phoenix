#![feature(negative_impls)]
#![feature(peer_credentials_unix_socket)]
#![feature(allocator_api)]
#![feature(nonnull_slice_from_raw_parts)]
#![feature(specialization)]
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
// GC
#![feature(drain_filter)]
// stub
#![feature(hash_drain_filter)]
// shmview drop
#![feature(maybe_uninit_uninit_array)]
#![feature(maybe_uninit_array_assume_init)]

use std::borrow::Borrow;
use std::env;
use std::path::PathBuf;

pub mod mrpc;
// TODO(wyj): change to pub(crate)
pub mod salloc;
pub mod transport;

// Re-exports
pub use transport::{cm, verbs, Error};

const DEFAULT_KOALA_PREFIX: &str = "/tmp/koala";
const DEFAULT_KOALA_CONTROL: &str = "koala-control.sock";

lazy_static::lazy_static! {
    pub(crate) static ref KOALA_PREFIX: PathBuf = {
        env::var("KOALA_PATH").map_or_else(|_| PathBuf::from(DEFAULT_KOALA_PREFIX), |p| {
            let path = PathBuf::from(p);
            assert!(path.is_dir(), "{path:?} is not a directly");
            path
        })
    };

    pub(crate) static ref KOALA_CONTROL_SOCK: PathBuf = {
        env::var("KOALA_CONTROL")
            .map_or_else(|_| PathBuf::from(DEFAULT_KOALA_CONTROL), PathBuf::from)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! _rx_recv_impl {
    ($srv:expr, $resp:path) => {
        match $srv.recv_comp()?.0 {
            Ok($resp) => Ok(()),
            Err(e) => Err(Error::Interface(stringify!($resp), e)),
            otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
        }
    };
    ($srv:expr, $resp:path, $ok_block:block) => {
        match $srv.recv_comp()?.0 {
            Ok($resp) => $ok_block,
            Err(e) => Err(Error::Interface(stringify!($resp), e)),
            otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
        }
    };
    ($srv:expr, $resp:path, $inst:ident, $ok_block:block) => {
        match $srv.recv_comp()?.0 {
            Ok($resp($inst)) => $ok_block,
            Err(e) => Err(Error::Interface(stringify!($resp), e)),
            otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
        }
    };
    ($srv:expr, $resp:path, $ok_block:block, $err:ident, $err_block:block) => {
        match $srv.recv_comp()?.0 {
            Ok($resp) => $ok_block,
            Err($err) => $err_block,
            otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
        }
    };
}

pub(crate) use _rx_recv_impl as rx_recv_impl;

// Get an owned structure from a borrow
pub trait FromBorrow<Borrowed> {
    fn from_borrow<T: Borrow<Borrowed>>(borrow: &T) -> Self;
}
