//! RPC data structures
use crate::Handle;

use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ShmBuf {
    pub shm_id: Handle,
    pub offset: u32,
    pub len: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SgList(pub Vec<ShmBuf>);

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RpcMsgType {
    Request,
    Response,
}

// #[repr(C)]
// #[derive(Debug, Clone)]
// pub struct RpcMessage {
//    pub rpc_id: usize,
//    pub msg_type: RpcMsgType,
//    pub sgl: Vec<ShmBuf>,
// }

pub trait Marshal {
    type Error;
    fn marshal(&self) -> Result<SgList, Self::Error>;
}

pub trait Unmarshal: Sized {
    type Error;
    fn unmarshal(sg_list: SgList) -> Result<Self, Self::Error>;
}

pub trait RpcMessage: Send {
    fn conn_id(&self) -> u32;
    fn func_id(&self) -> u32;
    fn call_id(&self) -> u64; // unique id
    fn len(&self) -> u64;
    fn is_request(&self) -> bool;
    fn marshal(&self) -> SgList;
}

/// # Safety
/// 
/// The zero-copy inter-process communication thing is beyond what the compiler
/// can check. The programmer must ensure that everything is fine.
pub unsafe trait SwitchAddressSpace {
    fn switch_address_space(&mut self);
}