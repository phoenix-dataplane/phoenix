use ipc::shmalloc::ShmPtr;

pub struct RawVec<T> {
    ptr: ShmPtr<T>,
    cap: usize
}
