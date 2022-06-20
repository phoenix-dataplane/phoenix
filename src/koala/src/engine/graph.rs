use std::ptr::Unique;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use interface::engine::EngineType;
use interface::rpc::MessageMeta;
use interface::Handle;
use ipc::mrpc::dp::{WrIdentifier, RECV_RECLAIM_BS};

// pub(crate) type IQueue = Receiver<Box<dyn RpcMessage>>;
// pub(crate) type OQueue = Sender<Box<dyn RpcMessage>>;
// TODO(cjr): change to non-blocking async-friendly SomeChannel<ShmPtr<dyn RpcMessage>>,

#[derive(Debug)]
pub(crate) struct RpcMessageTx {
    pub(crate) meta: Unique<MessageMeta>,
    pub(crate) addr_backend: usize,
}

#[derive(Debug)]
pub(crate) enum EngineTxMessage {
    RpcMessage(RpcMessageTx),
    ReclaimRecvBuf(Handle, [u32; RECV_RECLAIM_BS]),
}

pub(crate) type TxIQueue = UnboundedReceiver<EngineTxMessage>;
pub(crate) type TxOQueue = UnboundedSender<EngineTxMessage>;

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

pub(crate) type RxIQueue = UnboundedReceiver<EngineRxMessage>;
pub(crate) type RxOQueue = UnboundedSender<EngineRxMessage>;

pub(crate) trait Vertex {
    fn id(&self) -> &str;
    fn engine_type(&self) -> EngineType;
    fn tx_inputs(&mut self) -> &mut Vec<TxIQueue>;
    fn tx_outputs(&self) -> &Vec<TxOQueue>;
    fn rx_inputs(&mut self) -> &mut Vec<RxIQueue>;
    fn rx_outputs(&self) -> &Vec<RxOQueue>;
}
