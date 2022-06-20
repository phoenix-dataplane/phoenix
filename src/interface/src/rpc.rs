//! RPC data structures
use serde::{Deserialize, Serialize};

use crate::Handle;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RpcMsgType {
    Request,
    Response,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageMeta {
    pub conn_id: Handle,
    pub func_id: u32,
    pub call_id: u32,
    pub len: u64,
    pub msg_type: RpcMsgType,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MessageErased {
    pub meta: MessageMeta,
    pub shm_addr_app: usize,
    pub shm_addr_backend: usize,
}
