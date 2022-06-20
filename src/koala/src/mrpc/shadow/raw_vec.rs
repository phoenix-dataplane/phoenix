use ipc::shmalloc::ShmPtr;

pub struct RawVec<T> {
    pub ptr: ShmPtr<T>,
    pub cap: usize,
}
