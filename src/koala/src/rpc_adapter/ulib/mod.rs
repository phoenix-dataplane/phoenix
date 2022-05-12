use std::borrow::Borrow;
use std::io;

use thiserror::Error;

use crate::transport::rdma::{ApiError, DatapathError};

pub mod fp;
pub mod ucm;
pub mod uverbs;

#[derive(Error, Debug)]
pub(crate) enum Error {
    #[error("Error in RDMA API: {0}")]
    Api(#[from] ApiError),
    #[error("Datapath API Error: {0}")]
    Datapath(#[from] DatapathError),
    #[error("IO Error {0}")]
    Io(#[from] io::Error),
    // #[error("Interface error {0}: {1}")]
    // Interface(&'static str, interface::Error),
    #[error("No address is resolved")]
    NoAddrResolved,
    #[error("Connect failed: {0}")]
    Connect(ApiError),
}

// Get an owned structure from a borrow
pub(crate) trait FromBorrow<Borrowed> {
    fn from_borrow<T: Borrow<Borrowed>>(borrow: &T) -> Self;
}

use crate::transport::rdma::ops;

#[inline]
fn get_ops() -> &'static ops::Ops {
    use super::engine::TlStorage;
    use crate::engine::runtime::ENGINE_LS;
    ENGINE_LS.with(|els| {
        &els.borrow()
            .as_ref()
            .unwrap()
            .as_any()
            .downcast_ref::<TlStorage>()
            .unwrap()
            .ops
    })
}

// #[inline]
// fn get_ops_mut() -> &'static mut ops::Ops {
//     use super::engine::TlStorage;
//     use crate::engine::runtime::ENGINE_LS;
//     ENGINE_LS.with(|els| {
//         &mut els.borrow_mut()
//             .as_mut()
//             .unwrap()
//             .as_any()
//             .downcast_ref::<TlStorage>()
//             .unwrap()
//             .ops
//     })
// }
