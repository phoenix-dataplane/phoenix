//! The code below should be generated.
use std::mem;
use std::sync::Arc;

use ipc::shmalloc::{ShmPtr, SwitchAddressSpace};

use crate::salloc::state::Shared as SallocShared;
use crate::mrpc::marshal::{Marshal, Unmarshal, SgList, ShmBuf};

#[derive(Debug)]
pub struct HelloRequest {
    // TODO(wyj): replace Vec with mrpc's Vec
    pub name: Vec<u8>,
}

impl Marshal for HelloRequest {
    type Error = ();
    fn marshal(&self) -> Result<SgList, Self::Error> {
        let selfaddr = self as *const _ as usize;
        let vecptr = unsafe { (self as *const Self).cast::<usize>().read() };
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
    unsafe fn unmarshal(sg_list: SgList, salloc_state: &Arc<SallocShared>) -> Result<ShmPtr<Self>, Self::Error> {
        // TODO(cjr): double-check if the code below is correct.
        log::debug!("HelloReques, unmarshal, sg_list: {:0x?}", sg_list);
        if sg_list.0.len() != 2 {
            return Err(());
        }
        log::debug!("HelloReques, unmarshal, size_of::<Self>: {:0x?}", mem::size_of::<Self>());
        if sg_list.0[0].len != mem::size_of::<Self>() {
            return Err(());
        }
        let this_remote_addr = salloc_state.resource.query_app_addr(sg_list.0[0].ptr).unwrap();
        let mut this = ShmPtr::new(sg_list.0[0].ptr as *mut Self, this_remote_addr).unwrap();
        // TODO(wyj): replace this with mrpc's Vec, constructed from ShmPtr
        let vecaddr = sg_list.0[1].ptr;
        let vecptr = &mut this.as_mut().name as *mut _ as *mut usize;
        vecptr.write(vecaddr);
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
        let vecptr = unsafe { (self as *const Self).cast::<usize>().read() };
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
    unsafe fn unmarshal(sg_list: SgList, salloc_state: &Arc<SallocShared>) -> Result<ShmPtr<Self>, Self::Error> {
        // TODO(cjr): double-check if the code below is correct.
        if sg_list.0.len() != 2 {
            return Err(());
        }
        if sg_list.0[0].len != mem::size_of::<Self>() {
            return Err(());
        }
        let this_remote_addr = salloc_state.resource.query_app_addr(sg_list.0[0].ptr).unwrap();
        let mut this = ShmPtr::new(sg_list.0[0].ptr as *mut Self, this_remote_addr).unwrap();
        let vecaddr = sg_list.0[1].ptr;
        let vecptr = &mut this.as_mut().name as *mut _ as *mut usize;
        vecptr.write(vecaddr);
        Ok(this)
    }
}

unsafe impl SwitchAddressSpace for HelloRequest {
    fn switch_address_space(&mut self) {
        self.name.switch_address_space();
    }
}

unsafe impl SwitchAddressSpace for HelloReply {
    fn switch_address_space(&mut self) {
        self.name.switch_address_space();
    }
}
