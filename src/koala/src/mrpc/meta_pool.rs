use std::mem;
use std::ptr::Unique;

use fnv::FnvHashMap as HashMap;

use interface::rpc::MessageMeta;
use ipc::mrpc::dp::WrIdentifier;

use crate::resource::Error as ResourceError;

pub(crate) const META_BUFFER_SIZE: usize = 16384; // TODO(cjr): try 4096 or 256

/// A buffer holds the room for MessageMeta and optionally an entire message.
///
/// Format:
/// | meta | num_sge | value_len | lens[0] | lens[1] | ... | value[0] | value[1] | ... |
/// |  32  |    4    |     4     |                 EAGER_BUFFER_SIZE - 40              |
#[repr(C)]
#[derive(Debug, Clone)]
pub(crate) struct MetaBuffer {
    pub(crate) meta: MessageMeta,
    pub(crate) num_sge: u32,
    pub(crate) value_len: u32,
    pub(crate) lens_and_value: [u8; META_BUFFER_SIZE - 40],
}

mod sa {
    use super::*;
    use static_assertions::const_assert_eq;
    use std::mem::size_of;

    const_assert_eq!(size_of::<MetaBuffer>(), META_BUFFER_SIZE);
    const_assert_eq!(size_of::<Option<MetaBufferPtr>>(), 8);
}

impl MetaBuffer {
    #[inline]
    pub(crate) fn len(&self) -> usize {
        mem::size_of::<MessageMeta>()
            + self.num_sge as usize * mem::size_of::<u32>()
            + self.value_len as usize
    }

    #[inline]
    pub(crate) const fn lens_and_value_capacity() -> usize {
        META_BUFFER_SIZE - 40
    }
}

// Must be Send
#[derive(Debug, Clone, Copy)]
pub(crate) struct MetaBufferPtr(pub(crate) Unique<MetaBuffer>);

impl MetaBufferPtr {
    #[inline]
    fn new(ptr: Unique<MetaBuffer>) -> Self {
        MetaBufferPtr(ptr)
    }

    #[inline]
    pub(crate) fn as_meta_ptr(&self) -> *mut MessageMeta {
        self.0.as_ptr().cast()
    }
}

pub(crate) struct MetaBufferPool {
    #[allow(unused_variables)]
    buffer: Vec<MetaBuffer>,
    free: Vec<MetaBufferPtr>,
    used: HashMap<WrIdentifier, MetaBufferPtr>,
}

impl MetaBufferPool {
    pub(crate) fn new(cap: usize) -> Self {
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
    pub(crate) fn is_full(&self) -> bool {
        self.free.len() == self.buffer.capacity()
    }

    #[inline]
    pub(crate) fn obtain(&mut self, wr_id: WrIdentifier) -> Option<MetaBufferPtr> {
        self.free.pop().map(|buf| {
            self.used.insert(wr_id, buf);
            buf
        })
    }

    #[inline]
    pub(crate) fn release(&mut self, wr_id: WrIdentifier) -> Result<(), ResourceError> {
        let buf = self.used.remove(&wr_id).ok_or(ResourceError::NotFound)?;
        self.free.push(buf);
        Ok(())
    }
}
