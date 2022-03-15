//! RPC data structures
use serde::{Serialize, Deserialize};

use crate::Handle;

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

#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageMeta {
    pub conn_id: Handle,
    pub func_id: u32,
    pub call_id: u64,
    pub len: u64,
    pub msg_type: RpcMsgType,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MessageTemplateErased {
    pub meta: MessageMeta,
    pub shmptr: u64,
}
