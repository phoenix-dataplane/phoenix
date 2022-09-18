pub(crate) struct EncodedRecvBuffer {
    addr: usize,
    len: usize,
    _storage: Vec<u8>,
}

impl EncodedRecvBuffer {
    pub(crate) fn new(size: usize) -> EncodedRecvBuffer {
        let storage = Vec::with_capacity(size);
        assert_eq!(storage.capacity(), size);
        let ptr = storage.as_ptr() as *const u8;
        let addr = ptr.addr();
        EncodedRecvBuffer {
            addr,
            len: size,
            _storage: storage,
        }
    }
}

impl EncodedRecvBuffer {
    #[inline]
    pub(crate) fn addr(&self) -> usize {
        self.addr
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.len
    }
}
