use std::fmt;
use std::mem;
use std::ptr::Unique;

use fnv::FnvHashMap as HashMap;

use interface::rpc::{MessageMeta, RpcId};

use crate::resource::Error as ResourceError;

pub const META_BUFFER_SIZE: usize = 16384; // TODO(cjr): try 4096 or 256

/// A buffer holds the room for MessageMeta and optionally an entire message.
///
/// Format:
/// | meta | num_sge | value_len | lens[0] | lens[1] | ... | value[0] | value[1] | ... |
/// |  32  |    4    |     4     |                 EAGER_BUFFER_SIZE - 40              |
#[repr(C)]
#[derive(Clone)]
pub struct MetaBuffer {
    pub meta: MessageMeta,
    pub num_sge: u32,
    pub value_len: u32,
    pub length_delimited: [u8; META_BUFFER_SIZE - 40],
}

mod sa {
    use super::*;
    use static_assertions::const_assert_eq;
    use std::mem::size_of;

    const_assert_eq!(size_of::<MetaBuffer>(), META_BUFFER_SIZE);
    const_assert_eq!(size_of::<Option<MetaBufferPtr>>(), mem::size_of::<usize>());
}

impl fmt::Debug for MetaBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let print_count = self.value_buffer().len().min(64);
        f.debug_struct("MetaBuffer")
            .field("meta", &self.meta)
            .field("num_sge", &self.num_sge)
            .field("value_len", &self.value_len)
            .field("lens", &self.lens_buffer())
            .field("value", &(&self.value_buffer()[..print_count]))
            .finish()
    }
}

impl MetaBuffer {
    #[inline]
    pub fn len(&self) -> usize {
        mem::size_of::<MessageMeta>()
            + mem::size_of_val(&self.num_sge)
            + mem::size_of_val(&self.value_len)
            + self.value_start()
            + self.value_len as usize
    }

    #[inline]
    pub const fn capacity() -> usize {
        META_BUFFER_SIZE - 40
    }

    #[inline]
    pub const fn value_start(&self) -> usize {
        let start = self.num_sge as usize * mem::size_of::<u32>();
        assert!(start <= Self::capacity());
        start
    }

    #[inline]
    pub fn lens_buffer(&self) -> &[u8] {
        &self.length_delimited[..self.value_start()]
    }

    #[inline]
    pub fn value_buffer(&self) -> &[u8] {
        let base = self.value_start();
        let len = self.value_len as usize;

        assert!(
            base + len <= self.length_delimited.len(),
            "{} + {} <= {}",
            base,
            len,
            self.length_delimited.len()
        );

        &self.length_delimited[base..base + len]
    }
}

// Must be Send
#[derive(Debug, Clone, Copy)]
pub struct MetaBufferPtr(pub Unique<MetaBuffer>);

impl MetaBufferPtr {
    #[inline]
    fn new(ptr: Unique<MetaBuffer>) -> Self {
        MetaBufferPtr(ptr)
    }

    #[inline]
    pub fn as_meta_ptr(&self) -> *mut MessageMeta {
        self.0.as_ptr().cast()
    }
}

pub struct MetaBufferPool {
    #[allow(unused_variables)]
    buffer: Vec<MetaBuffer>,
    free: Vec<MetaBufferPtr>,
    used: HashMap<RpcId, MetaBufferPtr>,
}

impl MetaBufferPool {
    pub fn new(cap: usize) -> Self {
        let mut buffer = Vec::with_capacity(cap);

        let ptr: *mut MetaBuffer = buffer.as_mut_ptr();
        let mut free = Vec::with_capacity(cap);
        for i in 0..cap {
            free.push(MetaBufferPtr::new(
                Unique::new(unsafe { ptr.add(i) }).unwrap(),
            ));
        }

        MetaBufferPool {
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
    pub fn obtain(&mut self, rpc_id: RpcId) -> Option<MetaBufferPtr> {
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
