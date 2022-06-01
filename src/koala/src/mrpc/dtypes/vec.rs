use ipc::shmalloc::SwitchAddressSpace;
use super::raw_vec::RawVec;

pub struct Vec<T> {
    buf: RawVec<T>,
    len: usize,
}

impl<T> Vec<T> {
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub(crate) fn get_buf_addr(&self) -> usize {
        self.buf.ptr() as usize
    }

    pub(crate) unsafe fn update_buf_shmptr(&mut self, ptr: *mut T, addr_remote: usize) {
        self.buf.update_buf_shmptr(ptr, addr_remote);
    }

}

unsafe impl<T: SwitchAddressSpace> SwitchAddressSpace for Vec<T> {
    fn switch_address_space(&mut self) {
        let ptr = self.buf.ptr();
        let slice = unsafe { std::slice::from_raw_parts_mut(ptr, self.len) };
        for v in slice.iter_mut() {
            v.switch_address_space()
        }
        self.buf.switch_address_space();
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Vec<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ptr =  self.buf.ptr();
        let slice = unsafe { std::slice::from_raw_parts(ptr, self.len) };
        std::fmt::Debug::fmt(slice, f)
    }
}
