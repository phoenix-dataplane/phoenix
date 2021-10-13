use crate::shm::SharedMemory;
use slabmalloc::ZoneAllocator;
use spin::Mutex;
use std::alloc::Allocator;
use std::alloc::Layout;

pub(crate) struct ShmAlloc<'a>(Mutex<ShmAllocInternal<'a>>);

/// To use a ZoneAlloactor we require a lower-level allocator
/// (not provided by this crate) that can supply the allocator
/// with backing memory for `LargeObjectPage` and `ObjectPage` structs.
///
/// In our implementation we just request shared memory from the OS.
struct SharedPager {
    /// the name of the backed shared memory
    prefix: String,
    /// allocated shared memory
    shms: Vec<SharedMemory>,
}

impl SharedPager {
    const BASE_PAGE_SIZE: usize = 4096;
    const LARGE_PAGE_SIZE: usize = 2 * 1024 * 1024;

    // the backed file of this shared
    pub(crate) fn new(prefix: String) -> Self {
        SharedPager {
            prefix,
            shms: Vec::new(),
        }
    }

    /// Allocate a give `page_size`. Only called by `allocate_shared_page` and
    /// `allocate_shared_large_page`.
    fn acquire_shared_page(&mut self, page_size: usize) -> Option<*mut u8> {
        let layout = Layout::from_size_align(page_size, page_size).unwrap();
        let shm = SharedMemory::new("", layout.size()).ok()?;
        let ptr = shm.as_ptr();
        self.shms.push(shm);
        Some(ptr)
    }
}

pub struct ShmAllocInternal<'a> {
    slab: ZoneAllocator<'a>,
    pager: SharedPager,
}
