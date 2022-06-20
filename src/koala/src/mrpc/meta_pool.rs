use std::ptr::Unique;

use interface::rpc::MessageMeta;
use ipc::mrpc::dp::WrIdentifier;

use crate::resource::Error as ResourceError;

pub(crate) struct MessageMetaPool<const CAP: usize> {
    pub(crate) _meta_buffer: Box<[MessageMeta; CAP]>,
    pub(crate) freelist: Vec<Unique<MessageMeta>>,
    pub(crate) usedlist: fnv::FnvHashMap<WrIdentifier, Unique<MessageMeta>>,
}

impl<const CAP: usize> MessageMetaPool<CAP> {
    pub(crate) fn new() -> Self {
        let array = std::mem::MaybeUninit::uninit_array();
        let mut meta_buf = unsafe { Box::new(std::mem::MaybeUninit::array_assume_init(array)) };
        let meta_ptr: *mut interface::rpc::MessageMeta = meta_buf.as_mut_ptr();

        let mut freelist = Vec::with_capacity(128);
        for i in 0..meta_buf.len() {
            unsafe { freelist.push(Unique::new_unchecked(meta_ptr.add(i))) }
        }

        MessageMetaPool {
            _meta_buffer: meta_buf,
            freelist,
            usedlist: fnv::FnvHashMap::default(),
        }
    }

    #[inline]
    pub(crate) fn get(&mut self, wr_id: WrIdentifier) -> Option<Unique<MessageMeta>> {
        self.freelist.pop().map(|meta| {
            // Unique<MessageMeta> is copied here,
            // however, the pointer in meta_usedlist will not be dereferenced
            // until the corresponding message meta is released
            self.usedlist.insert(wr_id, meta);
            // SAFETY: it is the caller's responsbility that the Unique ptr will not be used
            // after the MessageMetaPool has been released (i.e., when mRPC engine shutdowns)
            meta
        })
    }

    #[inline]
    pub(crate) fn put(&mut self, wr_id: WrIdentifier) -> Result<(), ResourceError> {
        let meta_buf = self
            .usedlist
            .remove(&wr_id)
            .ok_or(ResourceError::NotFound)?;
        self.freelist.push(meta_buf);
        Ok(())
    }
}
