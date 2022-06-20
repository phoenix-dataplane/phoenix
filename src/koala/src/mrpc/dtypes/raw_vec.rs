use ipc::shmalloc::ShmPtr;

pub struct RawVec<T> {
    ptr: ShmPtr<T>,
    _cap: usize,
}

impl<T> RawVec<T> {
    #[inline]
    pub fn ptr_backend(&self) -> *mut T {
        self.ptr.as_ptr_backend()
    }

    pub(crate) unsafe fn update_buf_ptr(&mut self, ptr_app: *mut T, ptr_backend: *mut T) {
        self.ptr = ShmPtr::new(ptr_app, ptr_backend).unwrap();
    }
}
