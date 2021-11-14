#![feature(unix_socket_ancillary_data)]
pub use ipc_channel::ipc::{
    channel, IpcError, IpcOneShotServer as OneShotServer, IpcReceiver as Receiver,
    IpcReceiverSet as ReceiverSet, IpcSelectionResult, IpcSender as Sender, IpcSharedMemory,
    TryRecvError,
};

pub use ipc_channel::Error;

pub mod cmd;
pub mod dp;

pub mod unix;
pub use unix::{recv_fd, send_fd};
