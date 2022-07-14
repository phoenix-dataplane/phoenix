use std::ptr::Unique;

use interface::rpc::MessageMeta;
use mrpc_marshal::{SgE, UnmarshalError};

pub(crate) trait UnpackFromSgE: Sized {
    unsafe fn unpack(sge: &SgE) -> Result<Unique<Self>, UnmarshalError>;
}

impl UnpackFromSgE for MessageMeta {
    unsafe fn unpack(sge: &SgE) -> Result<Unique<Self>, UnmarshalError> {
        if sge.len != std::mem::size_of::<Self>() {
            return Err(UnmarshalError::SgELengthMismatch {
                expected: std::mem::size_of::<Self>(),
                actual: sge.len,
            });
        }
        let ptr = sge.ptr as *mut Self;
        let meta = Unique::new(ptr).unwrap();
        Ok(meta)
    }
}
