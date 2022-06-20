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
    pub(crate) fn get_buf_addr_backend(&self) -> usize {
        self.buf.ptr_backend() as usize
    }

    pub(crate) unsafe fn update_buf_ptr(&mut self, ptr_app: *mut T, ptr_backend: *mut T) {
        self.buf.update_buf_ptr(ptr_app, ptr_backend);
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Vec<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ptr = self.buf.ptr_backend();
        let slice = unsafe { std::slice::from_raw_parts(ptr, self.len) };
        std::fmt::Debug::fmt(slice, f)
    }
}
