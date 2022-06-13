use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use interface::engine::EngineType;
use interface::rpc::MessageErased;
use interface::Handle;
use ipc::mrpc::dp::RECV_RECLAIM_BS;

// pub(crate) type IQueue = Receiver<Box<dyn RpcMessage>>;
// pub(crate) type OQueue = Sender<Box<dyn RpcMessage>>;
// TODO(cjr): change to non-blocking async-friendly SomeChannel<ShmPtr<dyn RpcMessage>>,
pub(crate) enum EngineTxMessage {
    RpcMessage(MessageErased),
    ReclaimRecvBuf(Handle, [u32; RECV_RECLAIM_BS]),
}

pub(crate) type TxIQueue = UnboundedReceiver<EngineTxMessage>;
pub(crate) type TxOQueue = UnboundedSender<EngineTxMessage>;

#[derive(Debug)]
pub(crate) enum EngineRxMessage {
    RpcMessage(MessageErased),
    SendCompletion(Handle, u32),
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
