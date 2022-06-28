use std::ptr::Unique;

// use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use crossbeam::channel::{Receiver, Sender};

use interface::engine::EngineType;
use interface::rpc::MessageMeta;
use interface::Handle;
use ipc::mrpc::dp::{WrIdentifier, RECV_RECLAIM_BS};

use crate::mrpc::meta_pool::MetaBufferPtr;

// pub(crate) type IQueue = Receiver<Box<dyn RpcMessage>>;
// pub(crate) type OQueue = Sender<Box<dyn RpcMessage>>;
// TODO(cjr): change to non-blocking async-friendly SomeChannel<ShmPtr<dyn RpcMessage>>,

#[derive(Debug)]
pub(crate) struct RpcMessageTx {
    // Each RPC message is assigned an eager buffer
    pub(crate) meta_buf_ptr: MetaBufferPtr,
    pub(crate) addr_backend: usize,
}

#[derive(Debug)]
pub(crate) enum EngineTxMessage {
    RpcMessage(RpcMessageTx),
    ReclaimRecvBuf(Handle, [u32; RECV_RECLAIM_BS]),
}

#[derive(Debug)]
pub(crate) struct RpcMessageRx {
    pub(crate) meta: Unique<MessageMeta>,
    pub(crate) addr_app: usize,
    pub(crate) addr_backend: usize,
}

#[derive(Debug)]
pub(crate) enum EngineRxMessage {
    RpcMessage(RpcMessageRx),
    SendCompletion(WrIdentifier),
}

pub(crate) trait Vertex {
    fn id(&self) -> &str;
    fn engine_type(&self) -> EngineType;
    fn tx_inputs(&mut self) -> &mut Vec<TxIQueue>;
    fn tx_outputs(&self) -> &Vec<TxOQueue>;
    fn rx_inputs(&mut self) -> &mut Vec<RxIQueue>;
    fn rx_outputs(&self) -> &Vec<RxOQueue>;
}

pub(crate) type TxIQueue = Receiver<EngineTxMessage>;
pub(crate) type TxOQueue = Sender<EngineTxMessage>;

pub(crate) type RxIQueue = Receiver<EngineRxMessage>;
pub(crate) type RxOQueue = Sender<EngineRxMessage>;

pub(crate) fn create_channel<T>() -> (Sender<T>, Receiver<T>) {
    // tokio::sync::mpsc::unbounded_channel();
    crossbeam::channel::unbounded()
}

pub(crate) type SendError<T> = crossbeam::channel::SendError<T>;
pub(crate) type TryRecvError = crossbeam::channel::TryRecvError;
