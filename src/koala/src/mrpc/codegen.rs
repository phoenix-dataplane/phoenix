//! The code below should be generated.
use std::mem;
use crate::mrpc::marshal::{Marshal, Unmarshal, SgList, ShmBuf};

use unique::Unique;

#[derive(Debug)]
pub struct HelloRequest {
    pub name: Vec<u8>,
}

impl Marshal for HelloRequest {
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

impl Unmarshal for HelloRequest {
    type Error = ();
    unsafe fn unmarshal(sg_list: SgList) -> Result<Unique<Self>, Self::Error> {
        // TODO(cjr): double-check if the code below is correct.
        if sg_list.0.len() != 2 {
            return Err(());
        }
        if sg_list.0[0].len != mem::size_of::<Self> as usize {
            return Err(());
        }
        let mut this = Unique::new(sg_list.0[0].ptr as *mut Self).unwrap();
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
