//! Re-exports of shmem-ipc

pub use shmem_ipc::ringbuf::Error as ShmRingbufError;
pub use shmem_ipc::ringbuf::{Receiver as RingReceiver, Sender as RingSender};

pub use shmem_ipc::sharedring::{Receiver as ShmReceiver, Sender as ShmSender};
pub use shmem_ipc::Error as ShmIpcError;
