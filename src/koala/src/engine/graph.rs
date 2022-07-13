use std::ptr::Unique;

// use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use crossbeam::channel::{Receiver, Sender};

use interface::engine::EngineType;
use interface::rpc::{MessageMeta, RpcId, TransportStatus};
use interface::Handle;
use ipc::mrpc::dp::RECV_RECLAIM_BS;

use crate::mrpc::meta_pool::MetaBufferPtr;

// pub(crate) type IQueue = Receiver<Box<dyn RpcMessage>>;
// pub(crate) type OQueue = Sender<Box<dyn RpcMessage>>;
// TODO(cjr): change to non-blocking async-friendly SomeChannel<ShmPtr<dyn RpcMessage>>,

#[derive(Debug)]
pub struct RpcMessageTx {
    // Each RPC message is assigned a buffer for meta and optionally for its data
    pub(crate) meta_buf_ptr: MetaBufferPtr,
    pub(crate) addr_backend: usize,
}

#[derive(Debug)]
pub enum EngineTxMessage {
    RpcMessage(RpcMessageTx),
    ReclaimRecvBuf(Handle, [u32; RECV_RECLAIM_BS]),
}

#[derive(Debug)]
pub struct RpcMessageRx {
    pub(crate) meta: Unique<MessageMeta>,
    pub(crate) addr_app: usize,
    pub(crate) addr_backend: usize,
}

#[derive(Debug)]
pub enum EngineRxMessage {
    RpcMessage(RpcMessageRx),
    Ack(RpcId, TransportStatus),
}

pub trait Vertex {
    fn id(&self) -> &str;
    fn engine_type(&self) -> EngineType;
    fn tx_inputs(&mut self) -> &mut Vec<TxIQueue>;
    fn tx_outputs(&self) -> &Vec<TxOQueue>;
    fn rx_inputs(&mut self) -> &mut Vec<RxIQueue>;
    fn rx_outputs(&self) -> &Vec<RxOQueue>;
}

pub type TxIQueue = Receiver<EngineTxMessage>;
pub type TxOQueue = Sender<EngineTxMessage>;

pub type RxIQueue = Receiver<EngineRxMessage>;
pub type RxOQueue = Sender<EngineRxMessage>;

pub(crate) fn create_channel<T>() -> (Sender<T>, Receiver<T>) {
    // tokio::sync::mpsc::unbounded_channel();
    crossbeam::channel::unbounded()
}

pub(crate) type SendError<T> = crossbeam::channel::SendError<T>;
pub(crate) type TryRecvError = crossbeam::channel::TryRecvError;
