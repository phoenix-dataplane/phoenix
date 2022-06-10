// TODO(wyj): rewrite this file
use std::fmt;
use std::mem;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use interface::rpc::{MessageMeta, RpcMsgType};
use interface::Handle;

use ipc::shmalloc::ShmPtr;

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

impl MessageMeta {
    pub(crate) fn marshal(&self) -> ShmBuf {
        todo!()
    }
    
    pub(crate) unsafe fn unmarshal(sge: &ShmBuf) -> Self {
        todo!()
    }
}

// impl Marshal for MessageMeta {
//     type Error = ();
//     fn marshal(&self) -> Result<SgList, Self::Error> {
//         let selfptr = self as *const _ as usize;
//         let len = mem::size_of::<Self>();
//         Ok(SgList(vec![ShmBuf { ptr: selfptr, len }]))
//     }
// }

// impl Unmarshal for MessageMeta {
//     type Error = ();
//     unsafe fn unmarshal(
//         sg_list: SgList,
//         salloc_state: &Arc<SallocShared>,
//     ) -> Result<ShmPtr<Self>, Self::Error> {
//         if sg_list.0.len() != 1 {
//             return Err(());
//         }
//         if sg_list.0[0].len != mem::size_of::<Self>() {
//             return Err(());
//         }
//         let app_addr = salloc_state
//             .resource
//             .query_app_addr(sg_list.0[0].ptr)
//             .unwrap();
//         let ptr_backend = sg_list.0[0].ptr as *mut Self;
//         let ptr_app = ptr_backend.with_addr(app_addr);
//         let this =
//             ShmPtr::new(ptr_app, ptr_backend).unwrap();
//         Ok(this)
//     }
// }

