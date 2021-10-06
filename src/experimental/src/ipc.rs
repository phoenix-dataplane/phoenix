use crate::ringbuffer::RingBuffer;
use crate::shm::SharedMemory;
use core::alloc::Allocator;

pub enum IpcCommand {
    TestIpc,
}

pub enum IpcReply {
    TestIpc,
}

pub enum IpcError {
    ServerFailed,
}

pub type IpcResult<T> = Result<T, IpcError>;

struct SharedRegion(SharedMemory);

pub struct QueuePair<A: Allocator> {
    wq: RingBuffer<IpcCommand, A>,
    cq: RingBuffer<IpcResult<IpcReply>, A>,
}

pub struct IpcChannel<A: Allocator> {
    qp: Box<QueuePair<A>, A>,
    shared_region: SharedRegion,
    alloc: A,
}