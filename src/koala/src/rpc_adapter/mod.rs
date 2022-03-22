//! this engine translate RPC messages into transport-level work requests / completions
use thiserror::Error;

use crate::{mrpc::marshal::ShmBuf, resource::Error as ResourceError};

pub mod engine;
pub mod module;
pub mod state;
pub mod ulib;

#[derive(Error, Debug)]
#[error("rpc-adapter control path error")]
pub(crate) enum ControlPathError {
    // Below are errors that return to the user.
    #[error("Ulib error {0}")]
    Ulib(#[from] ulib::Error),
    #[error("Resource error: {0}")]
    Resource(#[from] ResourceError),

    // Below are errors that does not return to the user.
    #[error("Operation in progress")]
    InProgress,
    #[error("No Response is required. Note, this is not an error")]
    NoResponse,
    #[error("Ipc-channel TryRecvError")]
    IpcTryRecv,
    #[error("Send command error")]
    SendCommand,
    #[error("Service error: {0}")]
    Service(#[from] ipc::Error),
}

impl From<ControlPathError> for interface::Error {
    fn from(other: ControlPathError) -> Self {
        interface::Error::Generic(other.to_string())
    }
}

use std::sync::mpsc::SendError;
impl<T> From<SendError<T>> for ControlPathError {
    fn from(_other: SendError<T>) -> Self {
        Self::SendCommand
    }
}

#[derive(Error, Debug)]
pub(crate) enum DatapathError {
    #[error("Resource error: {0}")]
    Resource(#[from] ResourceError),
    #[error("Ulib error {0}")]
    Ulib(#[from] ulib::Error),
}

#[inline]
pub(crate) fn query_shm_offset(addr: usize) -> isize {
    use crate::engine::runtime::ENGINE_TLS;
    use engine::TlStorage;
    ENGINE_TLS.with(|tls| {
        let sge = ShmBuf {
            ptr: addr as _,
            len: 0,
        };
        let mr = tls
            .borrow()
            .as_ref()
            .unwrap()
            .downcast_ref::<TlStorage>()
            .unwrap()
            .state
            .resource()
            .query_mr(sge)
            .unwrap();
        mr.app_vaddr() as isize - mr.as_ptr() as isize
    })
}
