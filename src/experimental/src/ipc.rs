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

pub struct WorkQueue(RingBuffer<IpcCommand>);

pub struct CompletionQueue(RingBuffer<IpcResult<IpcReply>>);

pub struct QueuePair {
    wq: WorkQueue,
    cq: CompletionQueue,
}

pub struct IpcChannel<A: Allocator> {
    qp: QueuePair,
    shared_region: SharedRegion,
    alloc: A,
}