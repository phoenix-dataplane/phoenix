#![feature(negative_impls)]
#![feature(peer_credentials_unix_socket)]
use fnv::FnvHashMap as HashMap;
use std::borrow::Borrow;
use std::cell::RefCell;
use std::env;
use std::fs::File;
use std::io;
use std::os::unix::io::FromRawFd;
use std::os::unix::net::UCred;
use std::path::Path;

use lazy_static::lazy_static;
use thiserror::Error;
use uuid::Uuid;

use engine::SchedulingMode;
use ipc;
use ipc::transport::{cmd, control_plane, dp};
use ipc::unix::DomainSocket;

pub mod transport;
// Re-exports
pub use transport::{cm, verbs};

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO Error {0}")]
    Io(#[from] io::Error),
    #[error("Bincode error: {0}")]
    Bincode(#[from] bincode::Error),
    #[error("IPC send error: {0}")]
    IpcSend(ipc::Error),
    #[error("IPC recv error")]
    IpcRecv(ipc::IpcError),
    #[error("Interface error {0}: {1}")]
    Interface(&'static str, interface::Error),
    #[error("Control plane error {0}: {1}")]
    ControlPlane(&'static str, interface::Error),
    #[error("DomainSocket error: {0}")]
    UnixDomainSocket(#[from] ipc::unix::Error),
    #[error("Shared memory queue error: {0}")]
    ShmIpc(#[from] ipc::ShmIpcError),
    #[error("Shared memory queue ringbuf error: {0}")]
    ShmRingbuf(#[from] ipc::ShmRingbufError),
    #[error("ShmObject error: {0}")]
    ShmObj(#[from] ipc::shm::Error),
    #[error("No address is resolved")]
    NoAddrResolved,
    #[error("Expect a credential from the peer")]
    EmptyCredential,
    #[error("Credential mismatch {0:?} vs {1:?}")]
    CredentialMismatch(UCred, UCred),
    #[error("Connect failed: {0}")]
    Connect(interface::Error),
}

#[doc(hidden)]
#[macro_export]
macro_rules! _rx_recv_impl {
    ($rx:expr, $resp:path) => {
        match $rx.recv().map_err(Error::IpcRecv)?.0 {
            Ok($resp) => Ok(()),
            Err(e) => Err(Error::Interface(stringify!($resp), e)),
            otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
        }
    };
    ($rx:expr, $resp:path, $ok_block:block) => {
        match $rx.recv().map_err(Error::IpcRecv)?.0 {
            Ok($resp) => $ok_block,
            Err(e) => Err(Error::Interface(stringify!($resp), e)),
            otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
        }
    };
    ($rx:expr, $resp:path, $inst:ident, $ok_block:block) => {
        match $rx.recv().map_err(Error::IpcRecv)?.0 {
            Ok($resp($inst)) => $ok_block,
            Err(e) => Err(Error::Interface(stringify!($resp), e)),
            otherwise => panic!("Expect {}, found {:?}", stringify!($resp), otherwise),
        }
    };
    ($rx:expr, $resp:path, $ok_block:block, $err:ident, $err_block:block) => {
        match $rx.recv().map_err(Error::IpcRecv)?.0 {
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
