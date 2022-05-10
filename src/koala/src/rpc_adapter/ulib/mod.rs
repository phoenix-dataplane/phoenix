use std::borrow::Borrow;
use std::io;

use thiserror::Error;

pub mod fp;
pub mod ucm;
pub mod uverbs;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Service error: {0}")]
    Service(#[from] ipc::Error),
    #[error("IO Error {0}")]
    Io(#[from] io::Error),
    #[error("Interface error {0}: {1}")]
    Interface(&'static str, interface::Error),
    #[error("No address is resolved")]
    NoAddrResolved,
    #[error("Connect failed: {0}")]
    Connect(interface::Error),
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

use super::module::ServiceType;

#[inline]
fn get_service() -> &'static ServiceType {
    use super::engine::TlStorage;
    use crate::engine::runtime::ENGINE_LS;
    ENGINE_LS.with(|els| {
        &els.borrow()
            .as_ref()
            .unwrap()
            .as_any()
            .downcast_ref::<TlStorage>()
            .unwrap()
            .service
    })
}

use crate::transport::rdma::engine::TransportEngine;

#[inline]
fn get_api() -> &'static TransportEngine {
    use super::engine::TlStorage;
    use crate::engine::runtime::ENGINE_LS;
    ENGINE_LS.with(|els| {
        &els.borrow()
            .as_ref()
            .unwrap()
            .as_any()
            .downcast_ref::<TlStorage>()
            .unwrap()
            .api_engine
    })
}

#[inline]
fn get_cq_buffers() -> &'static super::state::CqBuffers {
    use super::engine::TlStorage;
    use crate::engine::runtime::ENGINE_LS;
    ENGINE_LS.with(|els| {
        &els.borrow()
            .as_ref()
            .unwrap()
            .as_any()
            .downcast_ref::<TlStorage>()
            .unwrap()
            .state
            .shared
            .cq_buffers
    })
}
