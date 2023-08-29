use std::ptr::Unique;

use mrpc_marshal::{SgE, UnmarshalError};
use phoenix_api::rpc::MessageMeta;

pub trait UnpackFromSgE: Sized {
    /// # Safety
    ///
    /// This operation may be zero-copy. Thus, the user must ensure the underlying data remain
    /// valid after unpacking.
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
