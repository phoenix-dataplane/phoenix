//! The code below should be generated.
use std::mem;
use interface::Handle;
use crate::mrpc::marshal::{Marshal, Unmarshal, SgList, ShmBuf};

#[derive(Debug)]
pub struct HelloRequest {
    pub name: Vec<u8>,
}

impl Marshal for HelloRequest {
    type Error = ();
    fn marshal(&self) -> Result<SgList, Self::Error> {
        let selfaddr = self as *const _ as usize;
        let vecptr = unsafe { (self as *const Self).cast::<usize>().read() };
        // let buf0 = ShmBuf {
        //     shm_id: Handle::INVALID,
        //     offset: 0,
        //     len: mem::size_of::<HelloRequest>() as u32,
        // };
        todo!();
    }
}

impl Unmarshal for HelloRequest {
    type Error = ();
    fn unmarshal(sg_list: SgList) -> Result<Self, Self::Error> {
        todo!();
    }
}


#[derive(Debug)]
pub struct HelloReply {
    pub name: Vec<u8>,
}
