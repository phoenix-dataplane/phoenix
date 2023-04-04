use std::fmt;
use std::mem;
use std::ptr::Unique;

use fnv::FnvHashMap as HashMap;

use phoenix_api::rpc::{MessageMeta, RpcId};

use crate::resource::Error as ResourceError;

/// The size of the [`MetaBuffer`] struct.
pub const META_BUFFER_SIZE: usize = 16384; // TODO(cjr): try 4096 or 256

/// A buffer that holds the room for [`MessageMeta`] and optionally the body of the message.
///
/// Format:
/// ```
/// | meta | num_sge | value_len | lens[0] | lens[1] | ... | value[0] | value[1] | ... |
/// |  40  |    4    |     4     |                 META_BUFFER_SIZE - 48               |
/// ```
#[repr(C)]
#[derive(Clone)]
pub struct MetaBuffer {
    /// The metadata of an RPC message.
    pub meta: MessageMeta,
    /// The number of disaggregated segments (or scatter-gather elements) the RPC message has.
    pub num_sge: u32,
    /// The length of the body of the RPC message inside this `MetaBuffer`.
    pub value_len: u32,
    /// The remaining raw bytes of the struct.
    pub length_delimited: [u8; META_BUFFER_SIZE - (mem::size_of::<MessageMeta>() + 8)],
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
    /// Returns the number of bytes contained in this `MetaBuffer`.
    #[inline]
    pub fn len(&self) -> usize {
        mem::size_of::<MessageMeta>()
            + mem::size_of_val(&self.num_sge)
            + mem::size_of_val(&self.value_len)
            + self.value_start()
            + self.value_len as usize
    }

    /// Returns the number of bytes the `MetaBuffer` can hold.
    #[inline]
    pub const fn capacity() -> usize {
        META_BUFFER_SIZE - 40
    }

    /// Returns the offset in bytes of the message to the beginning of `length_delimited`.
    ///
    /// # Panics
    ///
    /// Panics if the starting offset of the message exceeds the end of the buffer.
    #[inline]
    pub const fn value_start(&self) -> usize {
        let start = self.num_sge as usize * mem::size_of::<u32>();
        assert!(start <= Self::capacity());
        start
    }

    /// Returns a slice of u8 that represents the lengths of the segregated segments within the RPC
    /// message.
    #[inline]
    pub fn lens_buffer(&self) -> &[u8] {
        &self.length_delimited[..self.value_start()]
    }

    /// Returns a byte slice that represents buffer to the RPC message.
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

/// A `Unique` pointer to the a [`MetaBuffer`].
// Must be Send
#[derive(Debug, Clone, Copy)]
pub struct MetaBufferPtr(pub Unique<MetaBuffer>);

impl MetaBufferPtr {
    #[inline]
    fn new(ptr: Unique<MetaBuffer>) -> Self {
        MetaBufferPtr(ptr)
    }

    /// Returns an unsafe mutable pointer to the [RPC descriptor][`MessageMeta`].
    #[inline]
    pub fn as_meta_ptr(&self) -> *mut MessageMeta {
        self.0.as_ptr().cast()
    }
}

/// A pool of [`MetaBuffer`]s.
pub struct MetaBufferPool {
    #[allow(unused_variables)]
    buffer: Vec<MetaBuffer>,
    pub free: Vec<MetaBufferPtr>,
    used: HashMap<RpcId, MetaBufferPtr>,
}

impl MetaBufferPool {
    /// Creates a `MetaBufferPool` with `cap` free [`MetaBuffer`]s allocated.
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

    /// Returns `true` if the [`MetaBufferPool`] has no free buffers to obtain.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.free.len() == self.buffer.capacity()
    }

    /// Attempt to obtain a free [`MetaBuffer`] for a given `rpc_id`.
    ///
    /// Returns a [`MetaBufferPtr`] on success. Returns [`None`] if there is no free slots.
    #[inline]
    pub fn obtain(&mut self, rpc_id: RpcId) -> Option<MetaBufferPtr> {
        self.free.pop().map(|buf| {
            self.used.insert(rpc_id, buf);
            buf
        })
    }

    /// Release the [`MetaBuffer`] allocated for the `rpc_id`, making it available for
    /// future allocations.
    #[inline]
    pub fn release(&mut self, rpc_id: RpcId) -> Result<(), ResourceError> {
        let buf = self.used.remove(&rpc_id).ok_or(ResourceError::NotFound)?;
        self.free.push(buf);
        Ok(())
    }
}
