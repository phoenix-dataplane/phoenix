#![feature(unix_socket_ancillary_data)]
pub use ipc_channel::ipc::{
    channel, IpcError, IpcOneShotServer as OneShotServer, IpcReceiver as Receiver,
    IpcReceiverSet as ReceiverSet, IpcSelectionResult, IpcSender as Sender, IpcSharedMemory,
    TryRecvError,
};

pub use ipc_channel::Error;

pub use shmem_ipc::ringbuf::Error as ShmRingbufError;
pub use shmem_ipc::sharedring::{Receiver as ShmReceiver, Sender as ShmSender};
pub use shmem_ipc::Error as ShmIpcError;

pub mod buf;
pub mod cmd;
pub mod dp;

pub mod unix;
pub use unix::{recv_fd, send_fd};
