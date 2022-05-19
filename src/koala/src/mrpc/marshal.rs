// TODO(wyj): rewrite this file
use std::fmt;
use std::mem;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use interface::rpc::{MessageMeta, MessageTemplateErased, RpcMsgType};
use interface::Handle;

use ipc::shmalloc::{ShmPtr, SwitchAddressSpace};

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
    unsafe fn unmarshal(sg_list: SgList, salloc_state: &Arc<SallocShared>) -> Result<ShmPtr<Self>, Self::Error>;
}

impl Marshal for MessageMeta {
    type Error = ();
    fn marshal(&self) -> Result<SgList, Self::Error> {
        let selfptr = self as *const _ as usize;
        let len = mem::size_of::<Self>();
        Ok(SgList(vec![ShmBuf { ptr: selfptr, len }]))
    }
}

impl Unmarshal for MessageMeta {
    type Error = ();
    unsafe fn unmarshal(sg_list: SgList, salloc_state: &Arc<SallocShared>) -> Result<ShmPtr<Self>, Self::Error> {
        if sg_list.0.len() != 1 {
            return Err(());
        }
        if sg_list.0[0].len != mem::size_of::<Self>() {
            return Err(());
        }
        let this_remote_addr = salloc_state.resource.query_app_addr(sg_list.0[0].ptr).unwrap();
        let this = ShmPtr::new(sg_list.0[0].ptr as *mut Self, this_remote_addr).unwrap();
        Ok(this)
    }
}

// NOTE(wyj): Marshal should not be implemented for MessageTemplateErased
// impl Marshal for MessageTemplateErased {
//     type Error = ();
//     fn marshal(&self) -> Result<SgList, Self::Error> {
//         let selfptr = self as *const _ as usize;
//         let len = mem::size_of::<Self>();
//         let sgl = SgList(vec![ShmBuf { ptr: selfptr, len }]);
//         Ok(sgl)
//     }
// }

impl Unmarshal for MessageTemplateErased {
    type Error = ();
    unsafe fn unmarshal(mut sg_list: SgList, salloc_state: &Arc<SallocShared>) -> Result<ShmPtr<Self>, Self::Error> {
        if sg_list.0.len() <= 1 {
            return Err(());
        }

        let mut header_sgl = sg_list.0.remove(0);
        // header_sgl.len -= mem::size_of::<Unique<()>>();
        header_sgl.len = mem::size_of::<MessageMeta>();
        let meta = MessageMeta::unmarshal(SgList(vec![header_sgl]), salloc_state)?;
        let mut this = meta.cast::<Self>();
        let local_addr = sg_list.0[0].ptr as usize;
        this.as_mut().shm_addr = local_addr;
        let remote_msg_addr  = salloc_state.resource.query_app_addr(local_addr).unwrap();
        this.as_mut().shm_addr_remote = remote_msg_addr;
        Ok(this)
    }
}

pub(crate) trait RpcMessage: Send + SwitchAddressSpace {
    fn conn_id(&self) -> Handle;
    fn func_id(&self) -> u32;
    fn call_id(&self) -> u64; // unique id
    fn len(&self) -> u64;
    fn is_request(&self) -> bool;
    fn marshal(&self) -> SgList;
}

#[repr(C)]
#[derive(Debug)]
pub struct MessageTemplate<T> {
    meta: MessageMeta,
    val: ShmPtr<T>,
}

impl<T> MessageTemplate<T> {
    pub unsafe fn new(erased: MessageTemplateErased) -> ShmPtr<Self> {
        // TODO(cjr): double-check if it is valid at all to just conjure up an object on shm
        let this = ShmPtr::new(erased.shm_addr as *mut MessageTemplate<T>, erased.shm_addr_remote).unwrap();
        assert_eq!(this.as_ref().meta, erased.meta);
        debug!("this.as_ref.meta: {:?}", this.as_ref().meta);
        this
        // Self {
        //     meta: erased.meta,
        //     val: Unique::new(erased.shmptr as *mut T).unwrap(),
        // }
    }
}

impl<T: Marshal> Marshal for MessageTemplate<T> {
    type Error = <T as Marshal>::Error;
    fn marshal(&self) -> Result<SgList, Self::Error> {
        let selfptr = self as *const _ as usize;
        let len = mem::size_of::<Self>();
        let sge1 = ShmBuf { ptr: selfptr, len };
        let mut sgl = unsafe { self.val.as_ref() }.marshal()?;
        sgl.0.insert(0, sge1);
        // eprintln!("MessageTemplate<T>, marshal, sgl: {:0x?}", sgl);
        Ok(sgl)
    }
}

impl<T: Unmarshal> Unmarshal for MessageTemplate<T> {
    type Error = ();
    unsafe fn unmarshal(mut sg_list: SgList, salloc_state: &Arc<SallocShared>) -> Result<ShmPtr<Self>, Self::Error> {
        debug!("MessageTemplate<T>, unmarshal, sglist: {:0x?}", sg_list);
        if sg_list.0.len() <= 1 {
            return Err(());
        }
        let mut header_sgl = sg_list.0.remove(0);
        header_sgl.len -= mem::size_of::<ShmPtr<T>>();
        let meta = MessageMeta::unmarshal(SgList(vec![header_sgl]), salloc_state)?;
        debug!("MessageTemplate<T>, unmarshal, meta: {:?}", meta);
        let mut this = meta.cast::<Self>();
        debug!("MessageTemplate<T>, unmarshal, this: {:?}", this);
        let val = T::unmarshal(sg_list, salloc_state).or(Err(()))?;
        debug!("MessageTemplate<T>, unmarshal, val: {:?}", val);
        this.as_mut().val = val;
        Ok(this)
    }
}

impl<T: Send + Marshal + Unmarshal + SwitchAddressSpace> RpcMessage for MessageTemplate<T> {
    #[inline]
    fn conn_id(&self) -> Handle {
        self.meta.conn_id
    }
    #[inline]
    fn func_id(&self) -> u32 {
        self.meta.func_id
    }
    #[inline]
    fn call_id(&self) -> u64 {
        self.meta.call_id
    }
    #[inline]
    fn len(&self) -> u64 {
        self.meta.len
    }
    #[inline]
    fn is_request(&self) -> bool {
        self.meta.msg_type == RpcMsgType::Request
    }
    fn marshal(&self) -> SgList {
        // <Self as dyn Marshal>::marshal(self).unwrap()
        // <Self as Marshal<Error = <T as Marshal>::Error>>::marshal(self).unwrap()
        let span = trace_span!("marshal message template");
        let _enter = span.enter();
        (self as &dyn Marshal<Error = <T as Marshal>::Error>).marshal().unwrap()
    }
}


unsafe impl<T: SwitchAddressSpace> SwitchAddressSpace for MessageTemplate<T> {
    fn switch_address_space(&mut self) {
        unsafe { self.val.as_mut() }.switch_address_space();
    }
}
