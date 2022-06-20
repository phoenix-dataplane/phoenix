use std::ptr::Unique;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use interface::rpc::MessageMeta;

use ipc::ptr::ShmPtr;

#[derive(Error, Debug)]
pub enum MarshalError {
    // TBD
}

#[derive(Error, Debug)]
pub enum UnmarshalError {
    #[error("SgE length mismatch (expected={expected}, actual={actual})")]
    SgELengthMismatch { expected: usize, actual: usize },
    #[error("SgList underflow")]
    SgListUnderflow,
    #[error("could not find app addr: {0}")]
    AppAddrNotFound(#[from] crate::resource::Error),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SgE {
    pub ptr: usize,
    pub len: usize,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SgList(pub Vec<SgE>);

pub struct ExcavateContext<'a> {
    pub(crate) sgl: std::slice::Iter<'a, SgE>,
    pub(crate) salloc: &'a std::sync::Arc<crate::salloc::state::Shared>,
}

pub trait RpcMessage: Sized {
    fn marshal(&self) -> Result<SgList, MarshalError>;
    unsafe fn unmarshal<'a>(ctx: &mut ExcavateContext<'a>) -> Result<ShmPtr<Self>, UnmarshalError>;
    fn emplace(&self, sgl: &mut SgList) -> Result<(), MarshalError>;
    unsafe fn excavate<'a>(&mut self, ctx: &mut ExcavateContext<'a>) -> Result<(), UnmarshalError>;
    fn extent(&self) -> usize;
}

pub(crate) trait MetaUnpacking: Sized {
    unsafe fn unpack(sge: &SgE) -> Result<Unique<Self>, ()>;
}

impl MetaUnpacking for MessageMeta {
    unsafe fn unpack(sge: &SgE) -> Result<Unique<Self>, ()> {
        if sge.len != std::mem::size_of::<Self>() {
            return Err(());
        }
        let ptr = sge.ptr as *mut Self;
        let meta = Unique::new(ptr).unwrap();
        Ok(meta)
    }
}
