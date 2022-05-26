use std::alloc::{AllocError, Layout};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::mem;
use std::ptr::NonNull;

// use fnv::FnvHashMap as HashMap;
use lazy_static::lazy_static;
use memfd::Memfd;
use slabmalloc::{AllocablePage, HugeObjectPage, LargeObjectPage, ObjectPage, ZoneAllocator};

use ipc::salloc::cmd;

use super::region::SharedRegion;
use super::{Error, SA_CTX};

thread_local! {
    /// thread-local shared heap
    static TL_SHARED_HEAP: RefCell<SharedHeap> = RefCell::new(SharedHeap::new());
}

lazy_static! {
    static ref REGIONS: spin::Mutex<BTreeMap<usize, SharedRegion>> =
        spin::Mutex::new(BTreeMap::new());
}

struct SharedHeap {
    zone_allocator: ZoneAllocator<'static>,
}

impl SharedHeap {
    fn new() -> Self {
        SharedHeap {
            zone_allocator: ZoneAllocator::new(),
        }
    }

    fn allocate_shm(&self, len: usize) -> Result<SharedRegion, Error> {
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
                    Ok(SharedRegion::new(remote_addr, len,  align, file_off, memfd).unwrap())
                }
                Err(e) => Err(Error::Interface("AllocShm", e)),
                otherwise => panic!("Expect AllocShm, found {:?}", otherwise),
            }
        })
    }

    #[inline]
    fn allocate_huge_page(&mut self) -> Option<&'static mut HugeObjectPage<'static>> {
        match self.allocate_shm(HugeObjectPage::SIZE) {
            Ok(sr) => {
                let addr = sr.as_ptr().addr();
                assert!(addr & (HugeObjectPage::SIZE - 1) == 0, "addr: {:0x}", addr);
                let huge_object_page = unsafe { mem::transmute(addr) };
                REGIONS.lock().insert(addr, sr).ok_or(()).unwrap_err();
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
        // use mem::transmute to coerce an address to LargeObjectPage, make sure the size is
        // correct
        match self.allocate_shm(LargeObjectPage::SIZE) {
            Ok(sr) => {
                let addr = sr.as_ptr().addr();
                assert!(addr & (LargeObjectPage::SIZE - 1) == 0, "addr: {:0x}", addr);
                let large_object_page = unsafe { mem::transmute(addr) };
                REGIONS.lock().insert(addr, sr).ok_or(()).unwrap_err();
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
        match self.allocate_shm(ObjectPage::SIZE) {
            Ok(sr) => {
                let addr = sr.as_ptr().addr();
                assert!(addr & (ObjectPage::SIZE - 1) == 0, "addr: {:0x}", addr);
                let object_page = unsafe { mem::transmute(addr) };
                REGIONS.lock().insert(addr, sr).ok_or(()).unwrap_err();
                object_page
            }
            Err(e) => {
                eprintln!("allocate_page: {}", e);
                None
            }
        }
    }

    #[inline]
    fn release_huge_page(&mut self, _p: &'static mut HugeObjectPage<'static>) {
        todo!()
    }

    #[inline]
    fn release_large_page(&mut self, _p: &'static mut LargeObjectPage<'static>) {
        todo!()
    }

    #[inline]
    fn release_page(&mut self, _p: &'static mut ObjectPage<'static>) {
        todo!()
    }
}

impl Default for SharedHeap {
    fn default() -> Self {
        SharedHeap::new()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SharedHeapAllocator;

impl SharedHeapAllocator {
    #[inline]
    pub(crate) fn query_backend_addr(addr: usize) -> usize {
        let guard = REGIONS.lock();
        match guard.range(0..=addr).last() {
            Some(kv) => {
                assert!(
                    *kv.0 <= addr && *kv.0 + kv.1.len() > addr,
                    "addr: {:0x}, page_addr: {:0x}, page_len: {}",
                    addr,
                    *kv.0,
                    kv.1.len()
                );
                // retrieve offset within the mr
                let offset = addr & (kv.1.align - 1);
                kv.1.remote_addr() + offset
            }
            None => panic!(
                "addr: {:0x} not found in allocated pages, number of pages allocated: {}",
                addr,
                guard.len()
            ),
        }
    }
}

impl SharedHeapAllocator {
    pub(crate) fn allocate(&self, layout: Layout) -> Result<(NonNull<[u8]>, usize), AllocError> {
        use slabmalloc::{AllocationError, Allocator};
        match layout.size() {
            0..=ZoneAllocator::MAX_ALLOC_SIZE => {
                TL_SHARED_HEAP.with(|shared_heap| {
                    let mut shared_heap = shared_heap.borrow_mut();
                    match shared_heap.zone_allocator.allocate(layout) {
                        Ok(nptr) => {
                            let ptr = NonNull::slice_from_raw_parts(nptr, layout.size());
                            let addr_remote = Self::query_backend_addr(nptr.as_ptr() as usize);
                            Ok((ptr, addr_remote))
                        },
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
                            } else {
                                if let Some(huge_page) = shared_heap.allocate_huge_page() {
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
                            }
                            let nptr = shared_heap
                                .zone_allocator
                                .allocate(layout)
                                .expect("Should success after refill");
                            let addr_remote = Self::query_backend_addr(nptr.as_ptr() as usize);
                            let ptr = NonNull::slice_from_raw_parts(nptr, layout.size());
                            Ok((ptr, addr_remote))
                        }
                        Err(AllocationError::InvalidLayout) => {
                            eprintln!("Invalid layout: {:?}", layout);
                            Err(AllocError)
                        }
                    }
                })
            }
            _ => {
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
                            let nptr = NonNull::new(sr.as_mut_ptr()).unwrap();
                            REGIONS.lock().insert(addr, sr).ok_or(()).unwrap_err();
                            let ptr = NonNull::slice_from_raw_parts(nptr, layout.size());
                            let addr_remote = Self::query_backend_addr(nptr.as_ptr() as usize);
                            Ok((ptr, addr_remote))
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

    pub(crate) unsafe fn deallocate(&self, ptr: NonNull<u8>, addr_remote: usize, layout: Layout) {
        return;
        use slabmalloc::Allocator;
        match layout.size() {
            0..=ZoneAllocator::MAX_ALLOC_SIZE => {
                TL_SHARED_HEAP.with(|shared_heap| {
                    let mut shared_heap = shared_heap.borrow_mut();
                    shared_heap
                        .zone_allocator
                        .deallocate(ptr, layout)
                        .expect("Cannot deallocate");
                });

                // An proper reclamation strategy could be implemented here
                // to release empty pages back from the ZoneAllocator to the SharedHeap
            }
            _ => todo!("Handle object size larger than 1GB"),
        }
    }

    pub(crate) fn allocate_zeroed(&self, layout: Layout) -> Result<(NonNull<[u8]>, usize), AllocError> {
        let (ptr, addr_remote) = self.allocate(layout)?;
        // SAFETY: `alloc` returns a valid memory block
        unsafe { ptr.as_non_null_ptr().as_ptr().write_bytes(0, ptr.len()) }
        Ok((ptr, addr_remote))
    }

    pub(crate) unsafe fn grow(
        &self,
        ptr: NonNull<u8>,
        addr_remote: usize,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<(NonNull<[u8]>, usize), AllocError> {
        debug_assert!(
            new_layout.size() >= old_layout.size(),
            "`new_layout.size()` must be greater than or equal to `old_layout.size()`"
        );

        let (new_ptr, new_addr_remote) = self.allocate(new_layout)?;

        // SAFETY: because `new_layout.size()` must be greater than or equal to
        // `old_layout.size()`, both the old and new memory allocation are valid for reads and
        // writes for `old_layout.size()` bytes. Also, because the old allocation wasn't yet
        // deallocated, it cannot overlap `new_ptr`. Thus, the call to `copy_nonoverlapping` is
        // safe. The safety contract for `dealloc` must be upheld by the caller.
        unsafe {
            std::ptr::copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_mut_ptr(), old_layout.size());
            self.deallocate(ptr, addr_remote, old_layout);
        }

        Ok((new_ptr, new_addr_remote))
    }

    pub(crate) unsafe fn grow_zeroed(
        &self,
        ptr: NonNull<u8>,
        addr_remote: usize,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<(NonNull<[u8]>, usize), AllocError> {
        debug_assert!(
            new_layout.size() >= old_layout.size(),
            "`new_layout.size()` must be greater than or equal to `old_layout.size()`"
        );

        let (new_ptr, new_addr_remote) = self.allocate_zeroed(new_layout)?;

        // SAFETY: because `new_layout.size()` must be greater than or equal to
        // `old_layout.size()`, both the old and new memory allocation are valid for reads and
        // writes for `old_layout.size()` bytes. Also, because the old allocation wasn't yet
        // deallocated, it cannot overlap `new_ptr`. Thus, the call to `copy_nonoverlapping` is
        // safe. The safety contract for `dealloc` must be upheld by the caller.
        unsafe {
            std::ptr::copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_mut_ptr(), old_layout.size());
            self.deallocate(ptr, addr_remote, old_layout);
        }

        Ok((new_ptr, new_addr_remote))
    }

    pub(crate) unsafe fn shrink(
        &self,
        ptr: NonNull<u8>,
        addr_remote: usize,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<(NonNull<[u8]>, usize), AllocError> {
        debug_assert!(
            new_layout.size() <= old_layout.size(),
            "`new_layout.size()` must be smaller than or equal to `old_layout.size()`"
        );

        let (new_ptr, new_remote_addr) = self.allocate(new_layout)?;

        // SAFETY: because `new_layout.size()` must be lower than or equal to
        // `old_layout.size()`, both the old and new memory allocation are valid for reads and
        // writes for `new_layout.size()` bytes. Also, because the old allocation wasn't yet
        // deallocated, it cannot overlap `new_ptr`. Thus, the call to `copy_nonoverlapping` is
        // safe. The safety contract for `dealloc` must be upheld by the caller.
        unsafe {
            std::ptr::copy_nonoverlapping(ptr.as_ptr(), new_ptr.as_mut_ptr(), new_layout.size());
            self.deallocate(ptr, addr_remote, old_layout);
        }

        Ok((new_ptr, new_remote_addr))
    }
}