//! There are extensive FFI in this module. However, these foreign function only interface between
//! the backend compiler and the backend's plugin compiler, which we can guarantee to be the exact
//! same. Therefore, types such as SgList, ExcavateContext, MarshalError do not need to be #[repr(C)].
#![feature(strict_provenance)]
#![feature(core_intrinsics)]
#![feature(allocator_api)]
#![feature(alloc_layout_extra)]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use shm::ptr::ShmPtr;

pub mod emplacement;
pub mod shadow {
    use crate::alloc::PrivateHeap;

    pub type String = shm::string::String<PrivateHeap>;
    pub type Vec<T> = shm::vec::Vec<T, PrivateHeap>;
}

pub mod alloc {
    use std::alloc::{AllocError, GlobalAlloc, Layout, System};
    use std::ptr::NonNull;

    use shm::alloc::ShmAllocator;
    use shm::ptr::ShmNonNull;

    #[derive(Debug, Clone, Copy, Default)]
    pub struct PrivateHeap;

    unsafe impl ShmAllocator for PrivateHeap {
        #[inline]
        fn allocate(&self, layout: Layout) -> Result<ShmNonNull<[u8]>, AllocError> {
            // panic!("should not allocate");
            match layout.size() {
                0 => Ok(ShmNonNull::slice_from_raw_parts(
                    layout.dangling(),
                    layout.dangling(),
                    0,
                )),
                // SAFETY: `layout` is non-zero in size,
                size => unsafe {
                    // TODO(cjr): allocate a DMA-friendly memory.
                    // let raw_ptr = GlobalAlloc::alloc(&System, layout);
                    let raw_ptr = System.alloc(layout);
                    let ptr = NonNull::new(raw_ptr).ok_or(AllocError)?;
                    Ok(ShmNonNull::slice_from_raw_parts(ptr, ptr, size))
                },
            }
        }
        #[inline]
        fn deallocate(&self, ptr: ShmNonNull<u8>, layout: Layout) {
            // panic!("should not deallocate");
            let ptr = ptr.as_ptr_backend();
            unsafe { System.dealloc(ptr, layout) };
        }
    }
}

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
pub struct AddressNotFound(pub usize);

pub trait AddressArbiter {
    fn query_app_addr(&self, backend_addr: usize) -> Result<usize, AddressNotFound>;
}

impl<T: AddressArbiter> AddressArbiter for dyn AsRef<T> {
    fn query_app_addr(&self, backend_addr: usize) -> Result<usize, AddressNotFound> {
        <T as AddressArbiter>::query_app_addr(self.as_ref(), backend_addr)
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
    pub addr_arbiter: &'a A,
}

pub trait RpcMessage: Sized {
    fn marshal(&self) -> Result<SgList, MarshalError>;

    /// # Safety
    ///
    /// This operation may be zero-copy. Thus, the user must ensure the underlying data remain
    /// valid after unmarshalling.
    unsafe fn unmarshal<A: AddressArbiter>(
        ctx: &mut ExcavateContext<A>,
    ) -> Result<ShmPtr<Self>, UnmarshalError>;

    fn emplace(&self, sgl: &mut SgList) -> Result<(), MarshalError>;

    /// # Safety
    ///
    /// This operation may be zero-copy. Thus, the user must ensure the underlying data remain
    /// valid after excavating.
    unsafe fn excavate<A: AddressArbiter>(
        &mut self,
        ctx: &mut ExcavateContext<A>,
    ) -> Result<(), UnmarshalError>;

    fn extent(&self) -> usize;
}

// Implementations of AddressMap

#[derive(Error, Debug, Clone)]
#[error("address {0} already exists")]
pub struct AddressExists(pub usize);

// pub type AddressMap = NaiveAddressMap;
pub type AddressMap = NoopAddressMap;

#[allow(unused)]
pub struct NaiveAddressMap(spin::Mutex<BTreeMap<usize, ShmRecvMr>>);

impl AddressArbiter for NaiveAddressMap {
    fn query_app_addr(&self, backend_addr: usize) -> Result<usize, AddressNotFound> {
        let addr_map = self.0.lock();
        match addr_map.range(0..=backend_addr).last() {
            Some(kv) => {
                if kv.0 + kv.1.len >= backend_addr {
                    let offset = backend_addr & (kv.1.align - 1);
                    assert_eq!(
                        kv.1.ptr + offset,
                        backend_addr,
                        "Frontend and backend are mapped to the same virtual address. \
                        Do remove this assertation when this assumption does not held"
                    );
                    Ok(kv.1.ptr + offset)
                } else {
                    Err(AddressNotFound(backend_addr))
                }
            }
            None => Err(AddressNotFound(backend_addr)),
        }
    }
}

impl Default for NaiveAddressMap {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(unused)]
impl NaiveAddressMap {
    pub fn new() -> Self {
        NaiveAddressMap(spin::Mutex::new(BTreeMap::new()))
    }

    pub fn insert_addr_map(
        &self,
        local_addr: usize,
        remote_buf: ShmRecvMr,
    ) -> Result<(), AddressExists> {
        // SAFETY: it is the caller's responsibility to ensure the ShmMr is power-of-two aligned.

        // NOTE(wyj): local_addr points to the start of the recv_mr on backend side
        // the recv_mr on app side has the same length as the backend side
        // the length is logged in remote_buf
        self.0
            .lock()
            .insert(local_addr, remote_buf)
            .map_or_else(|| Ok(()), |_| Err(AddressExists(local_addr)))
    }
}

#[allow(unused)]
pub struct NoopAddressMap;

impl AddressArbiter for NoopAddressMap {
    fn query_app_addr(&self, backend_addr: usize) -> Result<usize, AddressNotFound> {
        // NoopAddressMap assumes we map the address using the same virtual address
        Ok(backend_addr)
    }
}

impl Default for NoopAddressMap {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(unused)]
impl NoopAddressMap {
    #[inline]
    pub fn new() -> Self {
        NoopAddressMap
    }

    #[inline]
    pub fn insert_addr_map(
        &self,
        _local_addr: usize,
        _remote_buf: ShmRecvMr,
    ) -> Result<(), AddressExists> {
        Ok(())
    }
}
