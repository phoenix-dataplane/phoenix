use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use interface::rpc::RpcId;
use lazy_static::lazy_static;

use slabmalloc::GLOBAL_PAGE_POOL;

// thread_local! {
//     // thread-local oustanding RPCs
//     // maps from RPC identifier (conn_id + call_id) to the message (RpcMessage) ID
//     // insert when RPC is posted, remove when corresponding ACK is polled
//     // each user app thread corresponds to a set of mRPC + RpcAdapter + SAlloc engines
//     // the thread which posts the RPC must also polls the corresponding ACK
//     pub(crate) static OUTSTANDING_RPC: RefCell<HashMap<RpcId, u64>> = RefCell::new(HashMap::new());
// }

lazy_static! {
    pub(crate) static ref MESSAGE_ID_COUNTER: AtomicU64 = AtomicU64::new(0);
    pub(crate) static ref CS_STUB_ID_COUNTER: AtomicU64 = AtomicU64::new(0);
}

lazy_static! {
    // pub(crate) static ref OBJECT_RECLAIMER: ObjectReclaimer = ObjectReclaimer::new();
    pub(crate) static ref PAGE_RECLAIMER_CTX: PageReclaimerContext =
        PageReclaimerContext::initialize();
}

pub(crate) struct PageReclaimerContext;

impl PageReclaimerContext {
    const SMALL_PAGE_RELEASE_THRESHOLD: usize = 4096;
    const SMALL_PAGE_RELEASE_RESERVE: usize = 2048;
    const LARGE_PAGE_RELEASE_THRESHOLD: usize = 256;
    const LARGE_PAGE_RELEASE_RESERVE: usize = 128;
    const HUGE_PAGE_RELEASE_THRESHOLD: usize = 0;
    const HUGE_PAGE_RELEASE_RESERVE: usize = 0;
    const RELEASE_INTERVAL_MS: u64 = 5000;

    async fn reclaim_task() {
        loop {
            GLOBAL_PAGE_POOL.release_small_pages(
                Self::SMALL_PAGE_RELEASE_THRESHOLD,
                Self::SMALL_PAGE_RELEASE_RESERVE,
            );
            GLOBAL_PAGE_POOL.release_large_pages(
                Self::LARGE_PAGE_RELEASE_THRESHOLD,
                Self::LARGE_PAGE_RELEASE_RESERVE,
            );
            GLOBAL_PAGE_POOL.release_huge_pages(
                Self::HUGE_PAGE_RELEASE_THRESHOLD,
                Self::HUGE_PAGE_RELEASE_RESERVE,
            );
            smol::Timer::after(std::time::Duration::from_millis(Self::RELEASE_INTERVAL_MS)).await;
        }
    }

    fn initialize() -> PageReclaimerContext {
        lazy_static::initialize(&GLOBAL_PAGE_POOL);
        let task = Self::reclaim_task();
        std::thread::spawn(move || smol::future::block_on(task));
        PageReclaimerContext
    }
}

// pub struct ObjectReclaimer {
//     // completed RPC count for each RpcMessage
//     rpc_completion_count: DashMap<u64, u64>,
//     storage: DashMap<u64, (Box<dyn Any + Send + Sync>, u64)>,
// }
// 
// impl ObjectReclaimer {
//     fn new() -> Self {
//         ObjectReclaimer {
//             rpc_completion_count: DashMap::new(),
//             storage: DashMap::new(),
//         }
//     }
// 
//     pub(crate) fn reclaim<T: 'static + Send + Sync>(
//         &self,
//         message: crate::alloc::Box<T>,
//         message_id: u64,
//         send_count: u64,
//     ) {
//         // we put the boxed MessageTemplate into GC by wrapping it as a trait object inside a std Box
//         // as CoerceUnsized cannot be implemented for ShmNonNull
//         let comp_cnt = match self.rpc_completion_count.get(&message_id) {
//             Some(entry) => *entry.value(),
//             None => 0,
//         };
//         if comp_cnt < send_count {
//             self.storage
//                 .insert(message_id, (Box::new(message), send_count));
//         }
//     }
// 
//     pub(crate) fn register_rpc_completion(&self, message_id: u64, cnt: u64) {
//         let comp_cnt = match self.rpc_completion_count.entry(message_id) {
//             Entry::Occupied(mut entry) => {
//                 let val = entry.get_mut();
//                 *val += cnt;
//                 *val
//             }
//             Entry::Vacant(entry) => {
//                 entry.insert(cnt);
//                 cnt
//             }
//         };
// 
//         if let Some(entry) = self.storage.get(&message_id) {
//             let send_cnt = entry.value().1;
//             if send_cnt == comp_cnt {
//                 self.storage.remove(&message_id);
//                 self.rpc_completion_count.remove(&message_id);
//             }
//         }
//     }
// }
