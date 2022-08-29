use std::borrow::Borrow;
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

#[inline]
pub(crate) fn get_ops() -> &'static Ops {
    use super::engine::ELS;
    ELS.with(|els| &els.borrow().as_ref().unwrap().ops)
}
