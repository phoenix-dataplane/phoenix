// TODO(wyj): rewrite this file
use std::fmt;
use std::mem;
use std::ptr::Unique;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use interface::rpc::MessageMeta;

use ipc::ptr::ShmPtr;

use crate::salloc::state::Shared as SallocShared;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct ShmBuf {
    pub ptr: usize,
    pub len: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct SgList(pub Vec<ShmBuf>);

pub(crate) trait Marshal {
    type Error: fmt::Debug;
    fn marshal(&self) -> Result<SgList, Self::Error>;
}

pub(crate) trait Unmarshal: Sized {
    type Error: fmt::Debug;
    // An unsafe method is a method whose caller must satisfy certain assertions.
    // Returns a ShmPtr<Self> to allow zerocopy unmarshal, and allow address space switching between backend and app.
    unsafe fn unmarshal(
        sg_list: &[ShmBuf],
        salloc_state: &Arc<SallocShared>,
    ) -> Result<ShmPtr<Self>, Self::Error>;
}

pub(crate) trait MetaUnpacking: Sized {
    unsafe fn unpack(sge: &ShmBuf) -> Result<Unique<Self>, ()>;
}

impl MetaUnpacking for MessageMeta {
    unsafe fn unpack(sge: &ShmBuf) -> Result<Unique<Self>, ()> {
        if sge.len != mem::size_of::<Self>() {
            return Err(());
        }
        let ptr = sge.ptr as *mut Self;
        let meta = Unique::new(ptr).unwrap();
        Ok(meta)
    }
}
