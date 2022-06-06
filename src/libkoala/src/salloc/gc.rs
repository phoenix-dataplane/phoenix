use std::cell::RefCell;
use std::collections::HashMap;
use std::any::Any;
use std::ptr::Unique;
use std::sync::atomic::AtomicU64;

use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use lazy_static::lazy_static;

use interface::Handle;
use slabmalloc::LargeObjectPage;

use crate::mrpc::stub::MessageTemplate;

use super::owner::AppOwned;

thread_local! {
    // thread-local oustanding work request
    // maps from WR identifier (conn_id + call_id) to the message (RpcMessage) ID and Client/Server ID
    // insert when WR is posted, remove when corresponding WC is polled
    // each user app thread corresponds to a set of mRPC + RpcAdapter + SAlloc engines
    // the thread which posts the WR must also polls the corresponding work request completion
    pub(crate) static OUTSTANDING_WR: RefCell<HashMap<(Handle, u32), (u64, u64)>> = RefCell::new(HashMap::new());
    // thread-local per-client/server outstanding WR count
    // maps from client/server ID to the number of outstanding WRs of that client/server.
    pub(crate) static CS_OUTSTANDING_WR_CNT: RefCell<HashMap<u64, u64>> = RefCell::new(HashMap::new());
}

lazy_static! {
    pub(crate) static ref GARBAGE_COLLECTOR: GarbageCollector = GarbageCollector::new();
    pub(crate) static ref MESSAGE_ID_COUNTER: AtomicU64 = AtomicU64::new(0);
    pub(crate) static ref CS_STUB_ID_COUNTER: AtomicU64 = AtomicU64::new(0);
}

pub struct GarbageCollector {
    // completed WR count for each RpcMessage
    wr_completion_count: DashMap<u64, u64>,
    storage: DashMap<u64, (std::boxed::Box<dyn Any + Send + Sync>, u64)>
}

impl GarbageCollector {
    fn new() -> Self {
        GarbageCollector { 
            wr_completion_count: DashMap::new(), 
            storage: DashMap::new()
        } 
    }

    pub(crate) fn collect<T: 'static + Send + Sync>(&self, message: crate::mrpc::alloc::Box<MessageTemplate<T, AppOwned>, AppOwned>, message_id: u64, send_count: u64) {
        // we put the boxed MessageTemplate into GC by wrapping it as a trait object inside a std Box
        // as CoerceUnsized cannot be implemented for ShmNonNull
        let comp_cnt = match self.wr_completion_count.get(&message_id) {
            Some(entry) => *entry.value(),
            None => 0
        };
        if comp_cnt < send_count {
            self.storage.insert(message_id, (Box::new(message), send_count));
        }
    }

    pub(crate) fn register_wr_completion(&self, message_id: u64) {
        let comp_cnt = match self.wr_completion_count.entry(message_id) {
            Entry::Occupied(mut entry) => {
                let val = entry.get_mut();
                *val += 1;
                *val
            },
            Entry::Vacant(entry) => {
                entry.insert(1);
                1
            },
        };
        
        let send_cnt = match self.storage.get(&message_id) {
            Some(entry) => entry.value().1,
            None => panic!("message already dropped")
        };

        if send_cnt == comp_cnt {
            self.storage.remove(&message_id);
            self.wr_completion_count.remove(&message_id);
        }
    }
}

pub struct GlobalPagePool {
    large_pages: Unique<LargeObjectPage<'static>>
}

impl GlobalPagePool {
    pub(crate) fn acquire_large_page(&self) -> &'static mut LargeObjectPage<'static> {
        let p = unsafe { &mut *self.large_pages.as_ptr() };
        p
    }
}