use std::fmt;
use std::mem;
use std::ptr::Unique;

use fnv::FnvHashMap as HashMap;

use interface::rpc::{MessageMeta, RpcId};

use koala::resource::Error as ResourceError;

pub const BUFFER_SIZE: usize = 8 * 1024 * 1024;

#[repr(C)]
#[derive(Clone)]
pub struct MessageBuffer {
    pub meta: MessageMeta,
    pub encoded_len: usize,
    pub encoded: [u8; BUFFER_SIZE - 40],
}

mod sa {
    use super::*;
    use static_assertions::const_assert_eq;
    use std::mem::size_of;

    const_assert_eq!(size_of::<MessageBuffer>(), BUFFER_SIZE);
    const_assert_eq!(
        size_of::<Option<MessageBufferPtr>>(),
        mem::size_of::<usize>()
    );
}

impl fmt::Debug for MessageBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let print_count = self.encoded_len.min(64);
        f.debug_struct("MetaBuffer")
            .field("meta", &self.meta)
            .field("encode_len", &self.encoded_len)
            .field("encoded", &(&self.encoded()[..print_count]))
            .finish()
    }
}

#[allow(unused)]
impl MessageBuffer {
    #[inline]
    pub fn len(&self) -> usize {
        mem::size_of::<MessageMeta>() + mem::size_of::<usize>() + self.encoded_len
    }

    #[inline]
    pub const fn capacity() -> usize {
        BUFFER_SIZE - 40
    }

    #[inline]
    pub fn encoded(&self) -> &[u8] {
        &self.encoded[..self.encoded_len]
    }

    #[inline]
    pub fn encoded_mut(&mut self) -> &mut [u8] {
        &mut self.encoded[..self.encoded_len]
    }
}

// Must be Send
#[derive(Debug, Clone, Copy)]
pub struct MessageBufferPtr(pub Unique<MessageBuffer>);

impl MessageBufferPtr {
    #[inline]
    fn new(ptr: Unique<MessageBuffer>) -> Self {
        MessageBufferPtr(ptr)
    }

    #[inline]
    pub fn meta_ptr(&self) -> *mut MessageMeta {
        self.0.as_ptr().cast()
    }
}

#[allow(unused)]
pub struct MessageBufferPool {
    #[allow(unused_variables)]
    buffer: Vec<MessageBuffer>,
    free: Vec<MessageBufferPtr>,
    used: HashMap<RpcId, MessageBufferPtr>,
}

#[allow(unused)]
impl MessageBufferPool {
    pub fn new(cap: usize) -> Self {
        let mut buffer = Vec::with_capacity(cap);

        let ptr: *mut MessageBuffer = buffer.as_mut_ptr();
        let mut free = Vec::with_capacity(cap);
        for i in 0..cap {
            free.push(MessageBufferPtr::new(
                Unique::new(unsafe { ptr.add(i) }).unwrap(),
            ));
        }

        MessageBufferPool {
            buffer,
            free,
            used: HashMap::default(),
        }
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.free.len() == self.buffer.capacity()
    }

    #[inline]
    pub fn obtain(&mut self, rpc_id: RpcId) -> Option<MessageBufferPtr> {
        self.free.pop().map(|buf| {
            self.used.insert(rpc_id, buf);
            buf
        })
    }

    #[inline]
    pub fn release(&mut self, rpc_id: RpcId) -> Result<(), ResourceError> {
        let buf = self.used.remove(&rpc_id).ok_or(ResourceError::NotFound)?;
        self.free.push(buf);
        Ok(())
    }
}
