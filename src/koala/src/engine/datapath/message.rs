use std::ptr::Unique;

use interface::rpc::{MessageMeta, RpcId, TransportStatus};
use interface::Handle;
use ipc::mrpc::dp::RECV_RECLAIM_BS;

// use crate::mrpc::meta_pool::MetaBufferPtr;
use super::meta_pool::MetaBufferPtr;

#[derive(Debug)]
pub struct RpcMessageTx {
    // Each RPC message is assigned a buffer for meta and optionally for its data
    pub meta_buf_ptr: MetaBufferPtr,
    pub addr_backend: usize,
}

#[derive(Debug)]
pub enum EngineTxMessage {
    RpcMessage(RpcMessageTx),
    ReclaimRecvBuf(Handle, [u32; RECV_RECLAIM_BS]),
}

#[derive(Debug)]
pub struct RpcMessageRx {
    pub meta: Unique<MessageMeta>,
    pub addr_app: usize,
    pub addr_backend: usize,
}

#[derive(Debug)]
pub enum EngineRxMessage {
    RpcMessage(RpcMessageRx),
    Ack(RpcId, TransportStatus),
}