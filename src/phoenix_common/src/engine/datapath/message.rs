use std::ptr::Unique;

use phoenix_api::rpc::{CallId, MessageMeta, RpcId, TransportStatus};
use phoenix_api::Handle;
use phoenix_api_mrpc::dp::RECV_RECLAIM_BS;

// use crate::mrpc::meta_pool::MetaBufferPtr;
use super::meta_pool::MetaBufferPtr;

// TODO(cjr): Should be repr(C)

#[derive(Debug)]
pub enum RpcMessageGeneral {
    TxMessage(EngineTxMessage),
    RxMessage(EngineRxMessage),
    Pass,
}

#[derive(Debug)]
pub struct RpcMessageTx {
    // Each RPC message is assigned a buffer for meta and optionally for its data
    pub meta_buf_ptr: MetaBufferPtr,
    pub addr_backend: usize,
}

impl RpcMessageTx {
    pub fn new(meta_buf_ptr: MetaBufferPtr, addr_backend: usize) -> RpcMessageTx {
        RpcMessageTx {
            meta_buf_ptr,
            addr_backend,
        }
    }
}

#[derive(Debug)]
pub enum EngineTxMessage {
    RpcMessage(RpcMessageTx),
    ReclaimRecvBuf(Handle, [CallId; RECV_RECLAIM_BS]),
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
    // (conn_id, status), we cannot know which rpc_id the receive corresponds
    RecvError(Handle, TransportStatus),
}
