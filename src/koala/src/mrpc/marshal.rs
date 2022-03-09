use serde::{Serialize, Deserialize};
use unique::Unique;

// use interface::Handle;
use interface::rpc::{MessageMeta, RpcMsgType, MessageTemplateErased};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SgList(pub Vec<ShmBuf>);

pub(crate) trait Marshal {
    type Error: std::fmt::Debug;
    fn marshal(&self) -> Result<SgList, Self::Error>;
}

pub(crate) trait Unmarshal: Sized {
    type Error: std::fmt::Debug;
    fn unmarshal(sg_list: SgList) -> Result<Self, Self::Error>;
}

// impl Marshal for MessageMeta {
//     type Error = ();
//     fn marshal(&self) -> Result<SgList, Self::Error> {
//         Ok(SgList(vec![ShmBuf { ptr: , len: }]))
//     }
// }
// 
// impl Unmarshal for MessageMeta {
//     type Error = ();
//     fn unmarshal(mut sg_list: SgList) -> Result<Self, Self::Error> {
//         if sg_list.0.len() != 1 {
//             return Err(());
//         }
//         unimplemented!()
//         // sg_list.0[0]
//     }
// }

pub(crate) trait RpcMessage: Send {
    fn conn_id(&self) -> u32;
    fn func_id(&self) -> u32;
    fn call_id(&self) -> u64; // unique id
    fn len(&self) -> u64;
    fn is_request(&self) -> bool;
    fn marshal(&self) -> SgList;
}

#[derive(Debug)]
pub struct MessageTemplate<T> {
    meta: MessageMeta,
    val: Unique<T>,
}

impl<T> MessageTemplate<T> {
    pub unsafe fn new(erased: MessageTemplateErased) -> Self {
        Self {
            meta: erased.meta,
            val: Unique::new(erased.shmptr as *mut T).unwrap(),
        }
    }
}

// impl<T: Marshal> Marshal for MessageTemplate<T> {
//     type Error = <T as Marshal>::Error;
//     fn marshal(&self) -> Result<SgList, Self::Error> {
//         unsafe { self.val.as_ref() }.marshal()
//     }
// }
// 
// impl<T: Unmarshal> Unmarshal for MessageTemplate<T> {
//     type Error = ();
//     fn unmarshal(mut sg_list: SgList) -> Result<Self, Self::Error> {
//         if sg_list.0.len() <= 1 {
//             return Err(());
//         }
//         let header_sgl = sg_list.0.remove(0);
//         let meta = MessageMeta::unmarshal(header_sgl)?;
//         let val = T::unmarshal(sg_list).or(Err(()))?;
//         Ok(Self {
//             meta,
//             val,
//         })
//     }
// }

impl<T: Send + Marshal + Unmarshal> RpcMessage for MessageTemplate<T> {
    #[inline]
    fn conn_id(&self) -> u32 { self.meta.conn_id }
    #[inline]
    fn func_id(&self) -> u32 { self.meta.func_id }
    #[inline]
    fn call_id(&self) -> u64 { self.meta.call_id }
    #[inline]
    fn len(&self) -> u64 { self.meta.len }
    #[inline]
    fn is_request(&self) -> bool { self.meta.msg_type == RpcMsgType::Request }
    fn marshal(&self) -> SgList {
        unsafe { self.val.as_ref() }.marshal().unwrap()
    }
}
