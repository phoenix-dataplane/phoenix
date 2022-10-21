use std::alloc::{AllocError, Layout};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::mem;
use std::num::NonZeroUsize;
use std::ptr::NonNull;

// use fnv::FnvHashMap as HashMap;
use lazy_static::lazy_static;
use memfd::Memfd;
use slabmalloc::GLOBAL_PAGE_POOL;
use slabmalloc::{AllocablePage, HugeObjectPage, LargeObjectPage, ObjectPage, ZoneAllocator};

use ipc::salloc::cmd;
use shm::ptr::ShmNonNull;

use super::backend::{Error, SA_CTX};
use region::WriteRegion;
use shm::alloc::ShmAllocator;

thread_local! {
    /// thread-local shared heap
    static TL_SHARED_HEAP: RefCell<WriteHeap> = RefCell::new(WriteHeap::new());
}

lazy_static! {
    pub(crate) static ref SHARED_HEAP_REGIONS: spin::Mutex<BTreeMap<usize, WriteRegion>> = {
        lazy_static::initialize(&super::gc::PAGE_RECLAIMER_CTX);
        spin::Mutex::new(BTreeMap::new())
    };
}

struct WriteHeap {
    zone_allocator: ZoneAllocator,
}

impl WriteHeap {
    fn new() -> Self {
        WriteHeap {
            zone_allocator: ZoneAllocator::new(),
        }
    }

    fn allocate_shm(&self, len: usize) -> Result<WriteRegion, Error> {
        assert!(len > 0);
        SA_CTX.with(|ctx| {
            // TODO(cjr): use a correct align
            let align = len;
            let req = cmd::Command::AllocShm(len, align);
            ctx.service.send_cmd(req)?;
            let fds = ctx.service.recv_fd()?;

            assert_eq!(fds.len(), 1);

            let memfd = Memfd::try_from_fd(fds[0]).map_err(|_| io::Error::last_os_error())?;
            let file_len = memfd.as_file().metadata()?.len() as usize;
            assert!(file_len >= len);

            match ctx.service.recv_comp().unwrap().0 {
                Ok(cmd::CompletionKind::AllocShm(remote_addr, file_off)) => {
                    Ok(WriteRegion::new(remote_addr, len, align, file_off, memfd).unwrap())
                }
                Err(e) => Err(Error::Interface("AllocShm", e)),
                otherwise => panic!("Expect AllocShm, found {:?}", otherwise),
            }
        })
    }

    #[inline]
    fn allocate_huge_page(&mut self) -> Option<&'static mut HugeObjectPage<'static>> {
        // take from global pool first
        if let Some(page) = GLOBAL_PAGE_POOL.acquire_huge_page() {
            return Some(page);
        }

        match self.allocate_shm(HugeObjectPage::SIZE) {
            Ok(sr) => {
                let addr = sr.as_ptr().addr();
                assert!(addr & (HugeObjectPage::SIZE - 1) == 0, "addr: {:0x}", addr);
                let huge_object_page = unsafe { mem::transmute(addr) };
                SHARED_HEAP_REGIONS
                    .lock()
                    .insert(addr, sr)
                    .ok_or(())
                    .unwrap_err();
                huge_object_page
            }
            Err(e) => {
                eprintln!("allocate_huge_page: {}", e);
                None
            }
        }
    }

    // slabmalloc must be supplied by fixed-size memory, aka `slabmalloc::AllocablePage`.
    #[inline]
    fn allocate_large_page(&mut self) -> Option<&'static mut LargeObjectPage<'static>> {
        if let Some(page) = GLOBAL_PAGE_POOL.acquire_large_page() {
            return Some(page);
        }

        // use mem::transmute to coerce an address to LargeObjectPage, make sure the size is
        // correct
        match self.allocate_shm(LargeObjectPage::SIZE) {
            Ok(sr) => {
                let addr = sr.as_ptr().addr();
                assert!(addr & (LargeObjectPage::SIZE - 1) == 0, "addr: {:0x}", addr);
                let large_object_page = unsafe { mem::transmute(addr) };
                SHARED_HEAP_REGIONS
                    .lock()
                    .insert(addr, sr)
                    .ok_or(())
                    .unwrap_err();
                large_object_page
            }
            Err(e) => {
                eprintln!("allocate_large_page: {}", e);
                None
            }
        }
    }

    #[inline]
    fn allocate_page(&mut self) -> Option<&'static mut ObjectPage<'static>> {
        if let Some(page) = GLOBAL_PAGE_POOL.acquire_small_page() {
            return Some(page);
        }

        match self.allocate_shm(ObjectPage::SIZE) {
            Ok(sr) => {
                let addr = sr.as_ptr().addr();
                assert!(addr & (ObjectPage::SIZE - 1) == 0, "addr: {:0x}", addr);
                let object_page = unsafe { mem::transmute(addr) };
                SHARED_HEAP_REGIONS
                    .lock()
                    .insert(addr, sr)
                    .ok_or(())
                    .unwrap_err();
                object_page
            }
            Err(e) => {
                eprintln!("allocate_page: {}", e);
                None
            }
        }
    }
}

impl Default for WriteHeap {
    fn default() -> Self {
        WriteHeap::new()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SharedHeapAllocator;

impl SharedHeapAllocator {
    #[inline]
    pub(crate) fn query_backend_addr(addr: usize, _align: usize) -> usize {
        addr
        // TODO(cjr, wyj): Change to HashMap
        // align can be obtained by addr + layout through ZoneAllocator
        // let guard = SHARED_HEAP_REGIONS.lock();
        // match guard.range(0..=addr).last() {
        //     Some(kv) => {
        //         assert!(
        //             *kv.0 <= addr && *kv.0 + kv.1.len() > addr,
        //             "addr: {:0x}, page_addr: {:0x}, page_len: {}",
        //             addr,
        //             *kv.0,
        //             kv.1.len()
        //         );
        //         // retrieve offset within the mr
        //         let offset = addr & (kv.1.align - 1);
        //         kv.1.remote_addr() + offset
        //     }
        //     None => panic!(
        //         "addr: {:0x} not found in allocated pages, number of pages allocated: {}",
        //         addr,
        //         guard.len()
        //     ),
        // }
    }
}

unsafe impl ShmAllocator for SharedHeapAllocator {
    fn allocate(&self, layout: Layout) -> Result<ShmNonNull<[u8]>, AllocError> {
        use slabmalloc::{AllocationError, Allocator};
        match layout.size() {
            0..=ZoneAllocator::MAX_ALLOC_SIZE => {
                TL_SHARED_HEAP.with(|shared_heap| {
                    let mut shared_heap = shared_heap.borrow_mut();

                    let result = match shared_heap.zone_allocator.allocate(layout) {
                        Ok(ptr_app) => {
                            let addr_backend =
                                Self::query_backend_addr(ptr_app.as_ptr().addr(), layout.align());
                            let ptr_backend =
                                ptr_app.with_addr(NonZeroUsize::new(addr_backend).unwrap());
                            let ptr = ShmNonNull::slice_from_raw_parts(
                                ptr_app,
                                ptr_backend,
                                layout.size(),
                            );

                            // TODO(wyj): release empty pages to global pool

                            Ok(ptr)
                        }
                        Err(err) => Err(err),
                    };

                    match result {
                        Ok(ptr) => Ok(ptr),
                        Err(AllocationError::OutOfMemory) => {
                            // refill the zone allocator
                            if layout.size() <= ZoneAllocator::MAX_BASE_ALLOC_SIZE {
                                if let Some(page) = shared_heap.allocate_page() {
                                    unsafe {
                                        shared_heap
                                            .zone_allocator
                                            .refill(layout, page)
                                            .unwrap_or_else(|_| {
                                                panic!("Cannot refill? layout: {:?}", layout)
                                            });
                                    }
                                } else {
                                    return Err(AllocError);
                                }
                            } else if layout.size() <= ZoneAllocator::MAX_LARGE_ALLOC_SIZE {
                                if let Some(large_page) = shared_heap.allocate_large_page() {
                                    let addr = large_page as *mut _ as usize;
                                    assert!(
                                        addr % 2097152 == 0,
                                        "addr: {:#0x?} is not huge page aligned",
                                        addr
                                    );
                                    unsafe {
                                        shared_heap
                                            .zone_allocator
                                            .refill_large(layout, large_page)
                                            .unwrap_or_else(|_| {
                                                panic!("Cannot refill? layout: {:?}", layout)
                                            });
                                    }
                                } else {
                                    return Err(AllocError);
                                }
                            } else if let Some(huge_page) = shared_heap.allocate_huge_page() {
                                let addr = huge_page as *mut _ as usize;
                                assert!(
                                    addr % (1024 * 1024 * 1024) == 0,
                                    "addr: {:#0x?} is not huge page aligned",
                                    addr
                                );
                                unsafe {
                                    shared_heap
                                        .zone_allocator
                                        .refill_huge(layout, huge_page)
                                        .unwrap_or_else(|_| {
                                            panic!("Cannot refill? layout: {:?}", layout)
                                        });
                                }
                            } else {
                                return Err(AllocError);
                            }
                            let ptr_app = shared_heap
                                .zone_allocator
                                .allocate(layout)
                                .expect("Should success after refill");
                            let addr_backend =
                                Self::query_backend_addr(ptr_app.as_ptr().addr(), layout.align());
                            let ptr_backend =
                                ptr_app.with_addr(NonZeroUsize::new(addr_backend).unwrap());
                            let ptr = ShmNonNull::slice_from_raw_parts(
                                ptr_app,
                                ptr_backend,
                                layout.size(),
                            );

                            // TODO(wyj): release empty pages to global pool

                            Ok(ptr)
                        }
                        Err(AllocationError::InvalidLayout) => {
                            eprintln!("Invalid layout: {:?}", layout);
                            Err(AllocError)
                        }
                    }
                })
            }
            _ => {
                // WARNING(wyj): dealloc will not be properly handled in this case
                tracing::error!(
                    "Requested: {} bytes. Please handle object size larger than {}",
                    layout.size(),
                    ZoneAllocator::MAX_ALLOC_SIZE
                );
                TL_SHARED_HEAP.with(|shared_heap| {
                    let shared_heap = shared_heap.borrow_mut();
                    let aligned_size = layout.align_to(4096).unwrap().pad_to_align().size();
                    match shared_heap.allocate_shm(aligned_size) {
                        Ok(sr) => {
                            let addr = sr.as_ptr().addr();
                            let ptr_app = NonNull::new(sr.as_mut_ptr()).unwrap();
                            SHARED_HEAP_REGIONS
                                .lock()
                                .insert(addr, sr)
                                .ok_or(())
                                .unwrap_err();
                            let addr_backend =
                                Self::query_backend_addr(ptr_app.as_ptr().addr(), layout.align());
                            let ptr_backend =
                                ptr_app.with_addr(NonZeroUsize::new(addr_backend).unwrap());
                            Ok(ShmNonNull::slice_from_raw_parts(
                                ptr_app,
                                ptr_backend,
                                layout.size(),
                            ))
                        }
                        Err(e) => {
                            eprintln!("allocate_shm: {}", e);
                            Err(AllocError)
                        }
                    }
                })
            }
        }
    }

    fn deallocate(&self, ptr: ShmNonNull<u8>, layout: Layout) {
        // backend deallocation is handled by SharedRegion::drop together with global GarbageCollector
        use slabmalloc::Allocator;
        match layout.size() {
            0..=ZoneAllocator::MAX_ALLOC_SIZE => {
                TL_SHARED_HEAP.with(|shared_heap| {
                    let mut shared_heap = shared_heap.borrow_mut();
                    shared_heap
                        .zone_allocator
                        .deallocate(ptr.to_raw_parts().0, layout)
                        .expect("Cannot deallocate");
                });
            }
            _ => todo!("Handle object size larger than 1GB"),
        }
    }
}

mod region {
    use std::ops::{Deref, DerefMut};
    use std::slice;

    use memfd::Memfd;
    use mmap::MmapFixed;

    use ipc::salloc::cmd::{Command, CompletionKind};
    use libphoenix::_rx_recv_impl as rx_recv_impl;

    use super::{Error, SA_CTX};

    // Shared region on sender heap
    #[derive(Debug)]
    pub(crate) struct WriteRegion {
        mmap: MmapFixed,
        remote_addr: usize,
        align: usize,
        _memfd: Memfd,
    }

    impl Deref for WriteRegion {
        type Target = [u8];
        fn deref(&self) -> &Self::Target {
            unsafe { slice::from_raw_parts(self.mmap.as_ptr().cast(), self.mmap.len()) }
        }
    }

    impl DerefMut for WriteRegion {
        fn deref_mut(&mut self) -> &mut Self::Target {
            unsafe { slice::from_raw_parts_mut(self.mmap.as_mut_ptr().cast(), self.mmap.len()) }
        }
    }

    impl Drop for WriteRegion {
        fn drop(&mut self) {
            (|| {
                SA_CTX.with(|ctx| {
                    let req = Command::DeallocShm(self.remote_addr);
                    ctx.service.send_cmd(req)?;
                    // TODO(wyj): do we really need to wait for completion here?
                    rx_recv_impl!(ctx.service, CompletionKind::DeallocShm)
                })
            })()
            .unwrap_or_else(|e| eprintln!("Dropping WriteRegion: {}", e));
        }
    }

    impl WriteRegion {
        pub(crate) fn new(
            remote_addr: usize,
            nbytes: usize,
            align: usize,
            file_off: i64,
            memfd: Memfd,
        ) -> Result<Self, Error> {
            tracing::debug!("WriteRegion::new, remote_addr: {:#0x?}", remote_addr);

            // Map to the same address as remote_addr, panic if it does not work
            let mmap = MmapFixed::new(remote_addr, nbytes, file_off as i64, memfd.as_file())?;

            Ok(WriteRegion {
                mmap,
                remote_addr,
                align,
                _memfd: memfd,
            })
        }

        #[inline]
        pub(crate) fn as_ptr(&self) -> *const u8 {
            self.mmap.as_ptr()
        }

        #[inline]
        pub(crate) fn as_mut_ptr(&self) -> *mut u8 {
            self.mmap.as_mut_ptr()
        }

        #[allow(unused)]
        #[inline]
        pub(crate) fn remote_addr(&self) -> usize {
            self.remote_addr
        }

        #[allow(unused)]
        #[inline]
        pub(crate) fn align(&self) -> usize {
            self.align
        }
    }
}
