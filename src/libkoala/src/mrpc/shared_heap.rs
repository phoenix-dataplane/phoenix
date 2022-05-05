use std::alloc::{AllocError, Allocator, Layout};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::mem;
use std::ptr::NonNull;

// use fnv::FnvHashMap as HashMap;
use lazy_static::lazy_static;
use memfd::Memfd;
use slabmalloc::{AllocablePage, LargeObjectPage, ObjectPage, ZoneAllocator};

use ipc::mrpc::cmd;

use crate::mrpc::{Error as MrpcError, MRPC_CTX};
use crate::verbs::MemoryRegion;

thread_local! {
    /// thread-local shared heap
    static TL_SHARED_HEAP: RefCell<SharedHeap> = RefCell::new(SharedHeap::new());
}

lazy_static! {
    static ref REGIONS: spin::Mutex<BTreeMap<usize, MemoryRegion<u8>>> =
        spin::Mutex::new(BTreeMap::new());
}

struct SharedHeap {
    // COMMET(cjr): currently, one shared heap must be exclusively associated with a protection domain.
    // pd: &'static ProtectionDomain,
    // vaddr -> MR
    zone_allocator: ZoneAllocator<'static>,
}

impl SharedHeap {
    // fn new() -> Self {
    //     // We use the first default pd. It should direct to the first NIC.
    //     // TODO(cjr): Add multi-NIC support!
    //     let default_pds = ProtectionDomain::default_pds();
    //     assert_eq!(default_pds.len(), 1);
    //     SharedHeap {
    //         pd: &default_pds[0],
    //         // mrs: HashMap::default(),
    //         zone_allocator: ZoneAllocator::new(),
    //     }
    // }

    fn new() -> Self {
        SharedHeap {
            zone_allocator: ZoneAllocator::new(),
        }
    }

    fn allocate_shm(&self, len: usize) -> Result<MemoryRegion<u8>, MrpcError> {
        assert!(len > 0);
        MRPC_CTX.with(|ctx| {
            let req = cmd::Command::AllocShm(len);
            ctx.service.send_cmd(req)?;
            let fds = ctx.service.recv_fd()?;

            assert_eq!(fds.len(), 1);

            let memfd = Memfd::try_from_fd(fds[0]).map_err(|_| io::Error::last_os_error())?;
            let file_len = memfd.as_file().metadata()?.len() as usize;
            assert!(file_len >= len);

            match ctx.service.recv_comp().unwrap().0 {
                Ok(cmd::CompletionKind::AllocShm(mr)) => Ok(MemoryRegion::new(
                    mr.pd,
                    mr.handle,
                    mr.rkey,
                    mr.vaddr,
                    len,
                    mr.file_off,
                    memfd,
                )
                .unwrap()),
                Err(e) => Err(MrpcError::Interface("AllocShm", e)),
                otherwise => panic!("Expect AllocShm, found {:?}", otherwise),
            }
        })
    }

    // slabmalloc must be supplied by fixed-size memory, aka `slabmalloc::AllocablePage`.
    #[inline]
    fn allocate_large_page(&mut self) -> Option<&'static mut LargeObjectPage<'static>> {
        // use mem::transmute to coerce an address to LargeObjectPage, make sure the size is
        // correct
        // match self.pd.allocate::<u8>(
        //     LargeObjectPage::SIZE,
        //     AccessFlags::LOCAL_WRITE | AccessFlags::REMOTE_READ | AccessFlags::REMOTE_WRITE,
        // ) {
        //     Ok(mr) => {
        //         let addr = mr.as_ptr() as usize;
        //         assert!(addr & (LargeObjectPage::SIZE - 1) == 0, "addr: {}", addr);
        //         let large_object_page = unsafe { mem::transmute(addr) };
        //         self.mrs.insert(addr, mr).ok_or(()).unwrap_err();
        //         large_object_page
        //     }
        //     Err(e) => {
        //         eprintln!("allocate_large_page: {}", e);
        //         None
        //     }
        // }
        match self.allocate_shm(LargeObjectPage::SIZE) {
            Ok(mr) => {
                let addr = mr.as_ptr() as usize;
                assert!(addr & (LargeObjectPage::SIZE - 1) == 0, "addr: {:0x}", addr);
                let large_object_page = unsafe { mem::transmute(addr) };
                REGIONS.lock().insert(addr, mr).ok_or(()).unwrap_err();
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
            Ok(mr) => {
                let addr = mr.as_ptr() as usize;
                assert!(addr & (ObjectPage::SIZE - 1) == 0, "addr: {:0x}", addr);
                let object_page = unsafe { mem::transmute(addr) };
                REGIONS.lock().insert(addr, mr).ok_or(()).unwrap_err();
                object_page
            }
            Err(e) => {
                eprintln!("allocate_page: {}", e);
                None
            }
        }
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
    pub(crate) fn query_shm_offset(addr: usize) -> isize {
        // The assumption does not hold for sure
        // let addr_aligned = addr & !(LargeObjectPage::SIZE - 1);
        // REGIONS.lock()[&addr_aligned].offset
        match REGIONS.lock().range(0..=addr).last() {
            Some(kv) => {
                assert!(
                    *kv.0 <= addr && *kv.0 + kv.1.len() > addr,
                    "addr: {:0x}, page_addr: {:0x}, page_len: {}",
                    addr,
                    *kv.0,
                    kv.1.len()
                );
                kv.1.remote_addr as isize - *kv.0 as isize
            }
            None => panic!(
                "addr: {:0x} not found in allocated pages, number of pages allocated: {}",
                addr,
                REGIONS.lock().len()
            ),
        }
    }
}

unsafe impl Allocator for SharedHeapAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        use slabmalloc::{AllocationError, Allocator};
        match layout.size() {
            0..=ZoneAllocator::MAX_ALLOC_SIZE => {
                TL_SHARED_HEAP.with(|shared_heap| {
                    let mut shared_heap = shared_heap.borrow_mut();
                    match shared_heap.zone_allocator.allocate(layout) {
                        Ok(nptr) => Ok(NonNull::slice_from_raw_parts(nptr, layout.size())),
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
                            } else {
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
                            }
                            let nptr = shared_heap
                                .zone_allocator
                                .allocate(layout)
                                .expect("Should success after refill");
                            Ok(NonNull::slice_from_raw_parts(nptr, layout.size()))
                        }
                        Err(AllocationError::InvalidLayout) => {
                            eprintln!("Invalid layout: {:?}", layout);
                            Err(AllocError)
                        }
                    }
                })
            }
            _ => {
                log::error!(
                    "Requested: {} bytes. Please handle object size larger than {}",
                    layout.size(), ZoneAllocator::MAX_ALLOC_SIZE
                );
                TL_SHARED_HEAP.with(|shared_heap| {
                    let shared_heap = shared_heap.borrow_mut();
                    let aligned_size = layout.align_to(4096).unwrap().pad_to_align().size();
                    match shared_heap.allocate_shm(aligned_size) {
                        Ok(mr) => {
                            let addr = mr.as_ptr() as usize;
                            let nptr = NonNull::new(mr.as_mut_ptr()).unwrap();
                            REGIONS.lock().insert(addr, mr).ok_or(()).unwrap_err();
                            Ok(NonNull::slice_from_raw_parts(nptr, layout.size()))
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

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
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
            _ => todo!("Handle object size larger than 2MB"),
        }
    }
}
