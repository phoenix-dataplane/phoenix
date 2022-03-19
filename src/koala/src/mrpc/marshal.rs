use std::mem;
use std::fmt;

use serde::{Deserialize, Serialize};
use unique::Unique;

use interface::rpc::{MessageMeta, MessageTemplateErased, RpcMsgType};
use interface::Handle;

// #[derive(Debug, Clone, Copy, Serialize, Deserialize)]
// pub(crate) struct ShmBuf {
//     pub shm_id: Handle,
//     pub offset: u32,
//     pub len: u32,
// }
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
    // Returns a Unique<Self> to allow zerocopy unmarshal.
    unsafe fn unmarshal(sg_list: SgList) -> Result<Unique<Self>, Self::Error>;
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
    unsafe fn unmarshal(sg_list: SgList) -> Result<Unique<Self>, Self::Error> {
        if sg_list.0.len() != 1 {
            return Err(());
        }
        if sg_list.0[0].len != mem::size_of::<Self>() {
            return Err(());
        }
        let this = Unique::new(sg_list.0[0].ptr as *mut Self).unwrap();
        Ok(this)
    }
}

impl Marshal for MessageTemplateErased {
    type Error = ();
    fn marshal(&self) -> Result<SgList, Self::Error> {
        let selfptr = self as *const _ as usize;
        let len = mem::size_of::<Self>();
        let sgl = SgList(vec![ShmBuf { ptr: selfptr, len }]);
        Ok(sgl)
    }
}

impl Unmarshal for MessageTemplateErased {
    type Error = ();
    unsafe fn unmarshal(mut sg_list: SgList) -> Result<Unique<Self>, Self::Error> {
        if sg_list.0.len() <= 1 {
            return Err(());
        }
        let mut header_sgl = sg_list.0.remove(0);
        header_sgl.len -= mem::size_of::<u64>();
        let meta = MessageMeta::unmarshal(SgList(vec![header_sgl]))?;
        let mut this = meta.cast::<Self>();
        this.as_mut().shmptr = sg_list.0[0].ptr as _;
        Ok(this)
    }
}

pub(crate) trait RpcMessage: Send {
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
    val: Unique<T>,
}

impl<T> MessageTemplate<T> {
    pub unsafe fn new(erased: MessageTemplateErased) -> Unique<Self> {
        let this = Unique::new(erased.shmptr as *mut MessageTemplate<T>).unwrap();
        assert_eq!(this.as_ref().meta, erased.meta);
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
        Ok(sgl)
    }
}

impl<T: Unmarshal> Unmarshal for MessageTemplate<T> {
    type Error = ();
    unsafe fn unmarshal(mut sg_list: SgList) -> Result<Unique<Self>, Self::Error> {
        if sg_list.0.len() <= 1 {
            return Err(());
        }
        let mut header_sgl = sg_list.0.remove(0);
        header_sgl.len -= mem::size_of::<Unique<T>>();
        let meta = MessageMeta::unmarshal(SgList(vec![header_sgl]))?;
        let mut this = meta.cast::<Self>();
        let val = T::unmarshal(sg_list).or(Err(()))?;
        this.as_mut().val = val;
        Ok(this)
    }
}

impl<T: Send + Marshal + Unmarshal> RpcMessage for MessageTemplate<T> {
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
        unsafe { self.val.as_ref() }.marshal().unwrap()
    }
}

/// # Safety
///
/// The zero-copy inter-process communication thing is beyond what the compiler
/// can check. The programmer must ensure that everything is fine.
pub unsafe trait SwitchAddressSpace {
    // An unsafe trait is unsafe to implement but safe to use.
    // The user of this trait does not need to satisfy any special condition.
    fn switch_address_space(&mut self);
}
