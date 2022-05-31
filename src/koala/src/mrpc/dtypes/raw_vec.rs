use ipc::shmalloc::{ShmPtr, SwitchAddressSpace};

pub struct RawVec<T> {
    ptr: ShmPtr<T>,
    _cap: usize
}

unsafe impl<T: SwitchAddressSpace> SwitchAddressSpace for RawVec<T> {
    fn switch_address_space(&mut self) {
        // RawVec does not handle T's switch_address_space; this is left for the users of RawVec
        self.ptr.switch_address_space();
    }
}

impl<T> RawVec<T> {
    #[inline]
    pub fn ptr(&self) -> *mut T {
        self.ptr.as_ptr()
    }

    pub(crate) unsafe fn update_buf_shmptr(&mut self, ptr: *mut T, addr_remote: usize) {
        self.ptr = ShmPtr::new(ptr, addr_remote as *mut T).unwrap();
    }
}