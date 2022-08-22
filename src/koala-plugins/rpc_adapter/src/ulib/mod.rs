use std::borrow::Borrow;
use std::cell::RefCell;
use std::io;

use thiserror::Error;

use transport_rdma::ops::Ops;
use transport_rdma::{ApiError, DatapathError};

#[allow(dead_code)]
pub(crate) mod fp;

#[allow(dead_code)]
pub(crate) mod ucm;

#[allow(dead_code)]
pub(crate) mod uverbs;

#[derive(Error, Debug)]
pub(crate) enum Error {
    #[error("Error in RDMA API: {0}")]
    Api(#[from] ApiError),
    #[error("Datapath API Error: {0}")]
    Datapath(#[from] DatapathError),
    #[error("IO Error {0}")]
    Io(#[from] io::Error),
    #[error("No address is resolved")]
    NoAddrResolved,
    #[error("Connect failed: {0}")]
    Connect(ApiError),
}

// Get an owned structure from a borrow
pub(crate) trait FromBorrow<Borrowed> {
    fn from_borrow<T: Borrow<Borrowed>>(borrow: &T) -> Self;
}

thread_local! {
    /// To emulate a thread local storage (TLS). This should be called engine-local-storage (ELS).
    pub(crate) static OPS: RefCell<Option<&'static Ops>> = RefCell::new(None);
}

#[inline]
fn get_ops() -> &'static Ops {
    OPS.with(|els| els.borrow().unwrap())
}
