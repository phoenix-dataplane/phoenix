use std::alloc::Global;
use std::any::Any;
use std::cell::RefCell;
use std::collections::LinkedList;
use std::collections::{HashMap, VecDeque};
use std::ptr::Unique;
use std::sync::atomic::AtomicU64;

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use lazy_static::lazy_static;

use interface::Handle;
use slabmalloc::{AllocablePage, HugeObjectPage, LargeObjectPage, ObjectPage};

use crate::mrpc::stub::MessageTemplate;

use super::heap::SHARED_HEAP_REGIONS;
use super::owner::AppOwned;

thread_local! {
    // thread-local oustanding work request
    // maps from WR identifier (conn_id + call_id) to the message (RpcMessage) ID and Client/Server ID
    // insert when WR is posted, remove when corresponding WC is polled
    // each user app thread corresponds to a set of mRPC + RpcAdapter + SAlloc engines
    // the thread which posts the WR must also polls the corresponding work request completion
    pub(crate) static OUTSTANDING_WR: RefCell<HashMap<(Handle, u32), (u64, u64)>> = RefCell::new(HashMap::new());
}

lazy_static! {
    pub(crate) static ref GARBAGE_COLLECTOR: GarbageCollector = GarbageCollector::new();
    pub(crate) static ref GLOBAL_PAGE_POOL: GlobalShreadHeapPagePool =
        GlobalShreadHeapPagePool::new();
    pub(crate) static ref MESSAGE_ID_COUNTER: AtomicU64 = AtomicU64::new(0);
    pub(crate) static ref CS_STUB_ID_COUNTER: AtomicU64 = AtomicU64::new(0);
}

pub struct GarbageCollector {
    // completed WR count for each RpcMessage
    wr_completion_count: DashMap<u64, u64>,
    storage: DashMap<u64, (std::boxed::Box<dyn Any + Send + Sync>, u64)>,
}

impl GarbageCollector {
    fn new() -> Self {
        GarbageCollector {
            wr_completion_count: DashMap::new(),
            storage: DashMap::new(),
        }
    }

    pub(crate) fn collect<T: 'static + Send + Sync>(
        &self,
        message: crate::mrpc::alloc::Box<MessageTemplate<T, AppOwned>, AppOwned>,
        message_id: u64,
        send_count: u64,
    ) {
        // we put the boxed MessageTemplate into GC by wrapping it as a trait object inside a std Box
        // as CoerceUnsized cannot be implemented for ShmNonNull
        let comp_cnt = match self.wr_completion_count.get(&message_id) {
            Some(entry) => *entry.value(),
            None => 0,
        };
        if comp_cnt < send_count {
            self.storage
                .insert(message_id, (Box::new(message), send_count));
        }
    }

    pub(crate) fn register_wr_completion(&self, message_id: u64, cnt: u64) {
        let comp_cnt = match self.wr_completion_count.entry(message_id) {
            Entry::Occupied(mut entry) => {
                let val = entry.get_mut();
                *val += cnt;
                *val
            }
            Entry::Vacant(entry) => {
                entry.insert(cnt);
                cnt
            }
        };

        if let Some(entry) = self.storage.get(&message_id) {
            let send_cnt = entry.value().1;
            if send_cnt == comp_cnt {
                self.storage.remove(&message_id);
                self.wr_completion_count.remove(&message_id);
            }
        }
    }
}


// Empty page pool should be maintained by our SharedHeapAllocator/GC
// instead of slabmalloc,
// as it is SharedHeapAllocator who refills ZoneAllocator with empty pages
pub struct GlobalShreadHeapPagePool {
    empty_small_pages: spin::Mutex<VecDeque<Unique<ObjectPage<'static>>>>,
    empty_large_pages: spin::Mutex<VecDeque<Unique<LargeObjectPage<'static>>>>,
    empty_huge_pages: spin::Mutex<VecDeque<Unique<HugeObjectPage<'static>>>>,

    used_small_pages: spin::Mutex<LinkedList<(Unique<ObjectPage<'static>>, usize)>>,
    used_large_pages: spin::Mutex<LinkedList<(Unique<LargeObjectPage<'static>>, usize)>>,
    used_huge_pages: spin::Mutex<LinkedList<(Unique<HugeObjectPage<'static>>, usize)>>,
}

impl GlobalShreadHeapPagePool {
    fn new() -> Self {
        GlobalShreadHeapPagePool {
            empty_small_pages: spin::Mutex::new(VecDeque::new()),
            empty_large_pages: spin::Mutex::new(VecDeque::new()),
            empty_huge_pages: spin::Mutex::new(VecDeque::new()),
            used_small_pages: spin::Mutex::new(LinkedList::new()),
            used_large_pages: spin::Mutex::new(LinkedList::new()),
            used_huge_pages: spin::Mutex::new(LinkedList::new()),
        }
    }

    fn check_small_page_assignments(&self) {
        let mut guard = self.used_small_pages.lock();
        let freed_pages = guard
            .drain_filter(|(page, obj_per_page)| unsafe { page.as_mut().is_empty(*obj_per_page) });

        let mut empty_gurad = self.empty_small_pages.lock();
        for (page, _) in freed_pages {
            empty_gurad.push_back(page);
        }
    }

    fn check_large_page_assignments(&self) {
        let mut guard = self.used_large_pages.lock();
        let freed_pages = guard
            .drain_filter(|(page, obj_per_page)| unsafe { page.as_mut().is_empty(*obj_per_page) });

        let mut empty_gurad = self.empty_large_pages.lock();
        for (page, _) in freed_pages {
            empty_gurad.push_back(page);
        }
    }

    fn check_huge_page_assignments(&self) {
        let mut guard = self.used_huge_pages.lock();
        let freed_pages = guard
            .drain_filter(|(page, obj_per_page)| unsafe { page.as_mut().is_empty(*obj_per_page) });

        let mut empty_gurad = self.empty_huge_pages.lock();
        for (page, _) in freed_pages {
            empty_gurad.push_back(page);
        }
    }

    pub(crate) fn acquire_small_page(&self) -> Option<&'static mut ObjectPage<'static>> {
        // TODO(wyj): do we really need to check each time?
        self.check_small_page_assignments();
        let page = self
            .empty_small_pages
            .lock()
            .pop_front()
            .map(|page| unsafe { &mut *page.as_ptr() });
        page
    }

    pub(crate) fn acquire_large_page(&self) -> Option<&'static mut LargeObjectPage<'static>> {
        self.check_large_page_assignments();
        let page = self
            .empty_large_pages
            .lock()
            .pop_front()
            .map(|page| unsafe { &mut *page.as_ptr() });
        page
    }

    pub(crate) fn acquire_huge_page(&self) -> Option<&'static mut HugeObjectPage<'static>> {
        self.check_huge_page_assignments();
        let page = self
            .empty_huge_pages
            .lock()
            .pop_front()
            .map(|page| unsafe { &mut *page.as_ptr() });
        page
    }

    pub(crate) fn recycle_small_page(
        &self,
        page: &'static mut ObjectPage<'static>,
        obj_per_page: usize,
    ) {
        if page.is_empty(obj_per_page) {
            self.empty_small_pages
                .lock()
                .push_back(unsafe { Unique::new_unchecked(page as *mut _) })
        } else {
            self.used_small_pages
                .lock()
                .push_back(unsafe { (Unique::new_unchecked(page as *mut _), obj_per_page) })
        }
    }

    
    // Caller must ensure provided pages are empty
    pub(crate) unsafe fn recycle_empty_small_pages<I: IntoIterator<Item = &'static mut ObjectPage<'static>>>(
        &self,
        pages: I,
    ) {
        let pages = pages.into_iter().map(|page| 
            unsafe { Unique::new_unchecked(page as *mut _) }
        );
        self.empty_small_pages.lock().extend(pages)
    }

    pub(crate) fn recycle_large_page(
        &self,
        page: &'static mut LargeObjectPage<'static>,
        obj_per_page: usize,
    ) {
        if page.is_empty(obj_per_page) {
            self.empty_large_pages
                .lock()
                .push_back(unsafe { Unique::new_unchecked(page as *mut _) })
        } else {
            self.used_large_pages
                .lock()
                .push_back(unsafe { (Unique::new_unchecked(page as *mut _), obj_per_page) })
        }
    }

    pub(crate) fn recycle_huge_page(
        &self,
        page: &'static mut HugeObjectPage<'static>,
        obj_per_page: usize,
    ) {
        if page.is_empty(obj_per_page) {
            self.empty_huge_pages
                .lock()
                .push_back(unsafe { Unique::new_unchecked(page as *mut _) })
        } else {
            self.used_huge_pages
                .lock()
                .push_back(unsafe { (Unique::new_unchecked(page as *mut _), obj_per_page) })
        }
    }
}

impl GlobalShreadHeapPagePool {
    const SMALL_PAGE_RELEASE_THRESHOLD: usize = 4096;
    const SMALL_PAGE_RELEASE_CNT: usize = 2048;
    const LARGE_PAGE_RELEASE_THRESHOLD: usize = 4096;
    const LARGE_PAGE_RELEASE_CNT: usize = 2048;
    const RELEASE_INTERVAL: u64 = 5000;

    pub(crate) async fn release_empty_pages(&self) {
        loop {
            {
                self.check_small_page_assignments();
                let mut guard = self.empty_small_pages.lock();
                if guard.len() > Self::SMALL_PAGE_RELEASE_THRESHOLD {
                    for empty_page in guard.drain(..Self::SMALL_PAGE_RELEASE_CNT) {
                        let addr = empty_page.as_ptr() as usize;
                        // tell backend to dealloc page
                        SHARED_HEAP_REGIONS
                            .lock()
                            .remove(&addr)
                            .expect("page already released");
                    }
                }
            }

            {
                self.check_large_page_assignments();
                let mut guard = self.empty_large_pages.lock();
                if guard.len() > Self::LARGE_PAGE_RELEASE_THRESHOLD {
                    for empty_page in guard.drain(..Self::LARGE_PAGE_RELEASE_CNT) {
                        let addr = empty_page.as_ptr() as usize;
                        // tell backend to dealloc page
                        SHARED_HEAP_REGIONS
                            .lock()
                            .remove(&addr)
                            .expect("page already released");
                    }
                }
            }
    
            {
                self.check_huge_page_assignments();
                // release all huge pages immediately
                let mut guard = self.empty_huge_pages.lock();
                for empty_page in guard.drain(..) {
                    let addr = empty_page.as_ptr() as usize;
                    // tell backend to dealloc page
                    SHARED_HEAP_REGIONS
                        .lock()
                        .remove(&addr)
                        .expect("page already released");
                }
            }
    
            smol::Timer::after(std::time::Duration::from_millis(Self::RELEASE_INTERVAL)).await;
        }
    }
}
