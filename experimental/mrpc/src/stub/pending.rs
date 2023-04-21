use std::cell::RefCell;

use fnv::FnvHashMap as HashMap;

use phoenix_api::rpc::RpcId;

use super::RpcData;
use crate::wref::{WRef, WRefOpaque};

/// A collection pending remote writable references. WRefs are added to this collection to prevent
/// from being release while the backend still using them.
#[derive(Debug, Default)]
pub(crate) struct PendingWRef {
    // pool: DashMap<RpcId, WRefOpaque, fnv::FnvBuildHasher>,
    pool: RefCell<HashMap<RpcId, WRefOpaque>>,
}

impl PendingWRef {
    #[inline]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub(crate) fn insert<T: RpcData>(&self, rpc_id: RpcId, wref: WRef<T>) {
        self.insert_opaque(rpc_id, wref.into_opaque())
    }

    #[inline]
    pub(crate) fn insert_opaque(&self, rpc_id: RpcId, wref_opaque: WRefOpaque) {
        self.pool.borrow_mut().insert(rpc_id, wref_opaque);
    }

    #[inline]
    pub(crate) fn remove(&self, rpc_id: &RpcId) {
        //assert!(self.pool.borrow_mut().remove(rpc_id).is_some());
        if self.pool.borrow_mut().remove(rpc_id).is_none() {
            // note: we should panic! But for now just ignore the error
            panic!("PendingWRef::remove: rpc_id {:?} not found", rpc_id);
        }
    }
}
