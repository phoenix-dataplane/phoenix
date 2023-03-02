#![feature(slice_index_methods)]
#![allow(missing_docs)]

pub mod handle;
pub use handle::{AsHandle, Handle};

pub mod error;
pub use error::Error;

pub mod buf;
pub mod net;

pub mod addrinfo;
pub mod engine;

#[cfg(feature = "mrpc")]
pub mod rpc;

#[cfg(feature = "salloc")]
pub use salloc;
#[cfg(feature = "transport")]
pub use transport;
