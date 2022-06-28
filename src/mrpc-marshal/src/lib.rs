#![feature(strict_provenance)]
#![feature(core_intrinsics)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use ipc::ptr::ShmPtr;

pub mod emplacement;
pub mod shadow;

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
    #[error("query app addr failed: {0}")]
    QueryAppAddr(#[from] AddressNotFound),
}

#[derive(Debug)]
pub struct ShmRecvMr {
    pub ptr: usize,
    pub len: usize,
    pub align: usize,
}

#[derive(Error, Debug, Clone)]
#[error("address {0} not found")]
pub struct AddressNotFound(usize);

pub trait AddressArbiter {
    fn query_app_addr(&self, backend_addr: usize) -> Result<usize, AddressNotFound>;
}

impl<T: AddressArbiter> AddressArbiter for std::rc::Rc<T> {
    fn query_app_addr(&self, backend_addr: usize) -> Result<usize, AddressNotFound> {
        <T as AddressArbiter>::query_app_addr(self.as_ref(), backend_addr)
    }
}

impl<T: AddressArbiter> AddressArbiter for std::sync::Arc<T> {
    fn query_app_addr(&self, backend_addr: usize) -> Result<usize, AddressNotFound> {
        <T as AddressArbiter>::query_app_addr(self.as_ref(), backend_addr)
    }
}

impl AddressArbiter for spin::Mutex<BTreeMap<usize, ShmRecvMr>> {
    fn query_app_addr(&self, backend_addr: usize) -> Result<usize, AddressNotFound> {
        let addr_map = self.lock();
        match addr_map.range(0..=backend_addr).last() {
            Some(kv) => {
                if kv.0 + kv.1.len >= backend_addr {
                    let offset = backend_addr & (kv.1.align - 1);
                    Ok(kv.1.ptr + offset)
                } else {
                    Err(AddressNotFound(backend_addr))
                }
            }
            None => Err(AddressNotFound(backend_addr)),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SgE {
    pub ptr: usize,
    pub len: usize,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SgList(pub Vec<SgE>);

pub struct ExcavateContext<'a, A: AddressArbiter> {
    pub sgl: std::slice::Iter<'a, SgE>,
    pub salloc: &'a A,
}

pub trait RpcMessage: Sized {
    fn marshal(&self) -> Result<SgList, MarshalError>;
    unsafe fn unmarshal<'a, A: AddressArbiter>(
        ctx: &mut ExcavateContext<'a, A>,
    ) -> Result<ShmPtr<Self>, UnmarshalError>;
    fn emplace(&self, sgl: &mut SgList) -> Result<(), MarshalError>;
    unsafe fn excavate<'a, A: AddressArbiter>(
        &mut self,
        ctx: &mut ExcavateContext<'a, A>,
    ) -> Result<(), UnmarshalError>;
    fn extent(&self) -> usize;
}
