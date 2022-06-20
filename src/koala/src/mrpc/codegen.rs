//! The code below should be generated.
use std::mem;
use std::sync::Arc;

use ipc::shmalloc::ShmPtr;

use crate::mrpc::dtypes::Vec;
use crate::mrpc::marshal::{Marshal, SgList, ShmBuf, Unmarshal};
use crate::salloc::state::Shared as SallocShared;

#[derive(Debug)]
pub struct HelloRequest {
    pub name: Vec<u8>,
}

impl Marshal for HelloRequest {
    type Error = ();
    fn marshal(&self) -> Result<SgList, Self::Error> {
        let selfaddr = self as *const _ as usize;
        let vecptr = self.name.get_buf_addr_backend();
        let buf0 = ShmBuf {
            ptr: selfaddr,
            len: mem::size_of::<Self>() as usize,
        };
        let buf1 = ShmBuf {
            ptr: vecptr,
            len: self.name.len() * mem::size_of::<u8>(),
        };
        Ok(SgList(vec![buf0, buf1]))
    }
}

impl Unmarshal for HelloRequest {
    type Error = ();
    unsafe fn unmarshal(
        sg_list: &[ShmBuf],
        salloc_state: &Arc<SallocShared>,
    ) -> Result<ShmPtr<Self>, Self::Error> {
        // TODO(cjr): double-check if the code below is correct.
        if sg_list.len() != 2 {
            return Err(());
        }
        if sg_list[0].len != mem::size_of::<Self>() {
            return Err(());
        }
        let this_app_addr = salloc_state
            .resource
            .query_app_addr(sg_list[0].ptr)
            .unwrap();
        let mut this =
            ShmPtr::new(this_app_addr as *mut Self, sg_list[0].ptr as *mut Self).unwrap();
        let vec_buf_addr_backend = sg_list[1].ptr;
        let vec_buf_addr_app = salloc_state
            .resource
            .query_app_addr(vec_buf_addr_backend)
            .unwrap();
        this.as_mut_backend()
            .name
            .update_buf_ptr(vec_buf_addr_app as *mut u8, vec_buf_addr_backend as *mut u8);
        Ok(this)
    }
}

#[derive(Debug)]
pub struct HelloReply {
    pub name: Vec<u8>,
}

impl Marshal for HelloReply {
    type Error = ();
    fn marshal(&self) -> Result<SgList, Self::Error> {
        // TODO(cjr): double-check if the code below is correct.
        let selfaddr = self as *const _ as usize;
        let vecptr = self.name.get_buf_addr_backend();
        let buf0 = ShmBuf {
            ptr: selfaddr,
            len: mem::size_of::<Self>() as usize,
        };
        let buf1 = ShmBuf {
            ptr: vecptr,
            len: self.name.len() * mem::size_of::<u8>(),
        };
        Ok(SgList(vec![buf0, buf1]))
    }
}

impl Unmarshal for HelloReply {
    type Error = ();
    unsafe fn unmarshal(
        sg_list: &[ShmBuf],
        salloc_state: &Arc<SallocShared>,
    ) -> Result<ShmPtr<Self>, Self::Error> {
        // TODO(cjr): double-check if the code below is correct.
        if sg_list.len() != 2 {
            return Err(());
        }
        if sg_list[0].len != mem::size_of::<Self>() {
            return Err(());
        }
        let this_app_addr = salloc_state
            .resource
            .query_app_addr(sg_list[0].ptr)
            .unwrap();
        let mut this =
            ShmPtr::new(this_app_addr as *mut Self, sg_list[0].ptr as *mut Self).unwrap();
        let vec_buf_addr_backend = sg_list[1].ptr;
        let vec_buf_addr_app = salloc_state
            .resource
            .query_app_addr(vec_buf_addr_backend)
            .unwrap();
        this.as_mut_backend()
            .name
            .update_buf_ptr(vec_buf_addr_app as *mut u8, vec_buf_addr_backend as *mut u8);
        Ok(this)
    }
}
