use std::ptr::Unique;

use interface::rpc::MessageMeta;

pub(crate) struct MessageMetaPool<const CAP: usize> {
    pub(crate) _meta_buffer: Box<[MessageMeta; CAP]>,
    pub(crate) meta_freelist: Vec<Unique<MessageMeta>>,
    pub(crate) meta_usedlist: fnv::FnvHashMap<u64, Unique<MessageMeta>>,
}

impl<const CAP: usize> MessageMetaPool<CAP> {
    pub(crate) fn new() -> Self {
        
    }
}