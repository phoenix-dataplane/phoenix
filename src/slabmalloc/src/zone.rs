//! A ZoneAllocator to allocate arbitrary object sizes (up to `ZoneAllocator::MAX_ALLOC_SIZE`)
//!
//! The ZoneAllocator achieves this by having many `SCAllocator`

use crate::{pool::GLOBAL_PAGE_POOL, *};

/// Creates an instance of a zone, we do this in a macro because we
/// re-use the code in const and non-const functions
///
/// We can get rid of this once the const fn feature is fully stabilized.
macro_rules! new_zone {
    () => {
        ZoneAllocator {
            // TODO(perf): We should probably pick better classes
            // rather than powers-of-two (see SuperMalloc etc.)
            small_slabs: [
                SCAllocator::new(1 << 3), // 8
                SCAllocator::new(1 << 4), // 16
                SCAllocator::new(1 << 5), // 32
                SCAllocator::new(1 << 6), // 64
                SCAllocator::new(1 << 7), // 128
                SCAllocator::new(1 << 8), // 256
            ],
            big_slabs: [
                SCAllocator::new(1 << 9),  // 512
                SCAllocator::new(1 << 10), // 1024
                SCAllocator::new(1 << 11), // 2048
                SCAllocator::new(1 << 12), // 4096
                SCAllocator::new(1 << 13), // 8192
                SCAllocator::new(1 << 14), // 16384
                SCAllocator::new(1 << 15), // 32767
                SCAllocator::new(1 << 16), // 65536
                SCAllocator::new(1 << 17), // 131072
            ],
            huge_slabs: [
                SCAllocator::new(1 << 18), // 262144
                SCAllocator::new(1 << 19), // 524288
                SCAllocator::new(1 << 20), // 1048576
                SCAllocator::new(1 << 21), // 2097152
                SCAllocator::new(1 << 22), // 4194304
                SCAllocator::new(1 << 23), // 8388608
                SCAllocator::new(1 << 24), // 16777216
                SCAllocator::new(1 << 25), // 33554432
                SCAllocator::new(1 << 26), // 67108864
            ],
        }
    };
}

/// A zone allocator for arbitrary sized allocations.
///
/// Has a bunch of `SCAllocator` and through that can serve allocation
/// requests for many different object sizes up to (MAX_SIZE_CLASSES) by selecting
/// the right `SCAllocator` for allocation and deallocation.
///
/// The allocator provides to refill functions `refill` and `refill_large`
/// to provide the underlying `SCAllocator` with more memory in case it runs out.
pub struct ZoneAllocator {
    small_slabs: [SCAllocator<'static, ObjectPage<'static>>; ZoneAllocator::MAX_BASE_SIZE_CLASSES],
    big_slabs:
        [SCAllocator<'static, LargeObjectPage<'static>>; ZoneAllocator::MAX_LARGE_SIZE_CLASSES],
    huge_slabs:
        [SCAllocator<'static, HugeObjectPage<'static>>; ZoneAllocator::MAX_HUGE_SIZE_CLASSES],
}

impl Default for ZoneAllocator {
    fn default() -> ZoneAllocator {
        new_zone!()
    }
}

enum Slab {
    Base(usize),
    Large(usize),
    Huge(usize),
    Unsupported,
}

impl ZoneAllocator {
    /// Maximum size that allocated within HugeObjectPages (1 GiB).
    /// This is also the maximum object size that this allocator can handle.
    pub const MAX_ALLOC_SIZE: usize = 1 << 26;

    /// Maximum size which is allocated with ObjectPages (4 KiB pages).
    ///
    /// e.g. this is 4 KiB - 80 bytes of meta-data.
    pub const MAX_BASE_ALLOC_SIZE: usize = 256;

    /// Maximum size which is allocated with LargeObjectPages (2 MiB pages).
    ///
    /// e.g. this is 2 MiB - 80 bytes of meta-data.
    pub const MAX_LARGE_ALLOC_SIZE: usize = 131_072;

    /// How many allocators of type SCAllocator<ObjectPage> we have.
    const MAX_BASE_SIZE_CLASSES: usize = 6;

    /// How many allocators of type SCAllocator<LargeObjectPage> we have.
    const MAX_LARGE_SIZE_CLASSES: usize = 9;

    /// How many allocators of type SCAllocator<HugeObjectPage> we have.
    const MAX_HUGE_SIZE_CLASSES: usize = 9;

    #[cfg(feature = "unstable")]
    pub const fn new() -> ZoneAllocator {
        new_zone!()
    }

    #[cfg(not(feature = "unstable"))]
    pub fn new() -> ZoneAllocator<'a> {
        new_zone!()
    }

    /// Return maximum size an object of size `current_size` can use.
    ///
    /// Used to optimize `realloc`.
    #[cfg(feature = "unstable")]
    pub fn get_max_size(current_size: usize) -> Option<usize> {
        match current_size {
            0..=8 => Some(8),
            9..=67_108_864 => current_size.checked_next_power_of_two(),
            _ => None,
        }
    }

    #[cfg(not(feature = "unstable"))]
    pub fn get_max_size(current_size: usize) -> Option<usize> {
        match current_size {
            0..=8 => Some(8),
            9..=16 => Some(16),
            17..=32 => Some(32),
            33..=64 => Some(64),
            65..=128 => Some(128),
            129..=256 => Some(256),
            257..=512 => Some(512),
            513..=1024 => Some(1024),
            1025..=2048 => Some(2048),
            2049..=4096 => Some(4096),
            4097..=8192 => Some(8192),
            8193..=16384 => Some(16384),
            16385..=32767 => Some(32767),
            32768..=65536 => Some(65536),
            65537..=131_072 => Some(131_072),
            131_073..=262_144 => Some(262_144),
            262_145..=524_288 => Some(524_288),
            524_289..=1_048_576 => Some(1_048_576),
            1_048_577..=2_097_152 => Some(2_097_152),
            2_097_153..=4_194_304 => Some(4_194_304),
            4_194_305..=8_388_608 => Some(8_388_608),
            8_388_609..=16_777_216 => Some(16_777_216),
            16_777_217..=33_554_432 => Some(33_554_432),
            33_554_433..=67_108_864 => Some(67_108_864),
            _ => None,
        }
    }

    /// Figure out index into zone array to get the correct slab allocator for that size.
    #[cfg(feature = "unstable")]
    fn get_slab(requested_size: usize) -> Slab {
        match requested_size {
            0..=8 => Slab::Base(0),
            9..=256 => Slab::Base((requested_size - 1).wrapping_shr(2).ilog2() as _),
            257..=131_072 => Slab::Large((requested_size - 1).wrapping_shr(8).ilog2() as _),
            131_073..=67_108_864 => Slab::Huge((requested_size - 1).wrapping_shr(17).ilog2() as _),
            _ => Slab::Unsupported,
        }
    }

    #[cfg(not(feature = "unstable"))]
    fn get_slab(requested_size: usize) -> Slab {
        // TODO(cjr): finish this
        match requested_size {
            0..=8 => Slab::Base(0),
            9..=16 => Slab::Base(1),
            17..=32 => Slab::Base(2),
            33..=64 => Slab::Base(3),
            65..=128 => Slab::Base(4),
            129..=256 => Slab::Base(5),
            257..=512 => Slab::Large(0),
            513..=1024 => Slab::Large(1),
            1025..=2048 => Slab::Large(2),
            2049..=4096 => Slab::Large(3),
            4097..=8192 => Slab::Large(4),
            8193..=16384 => Slab::Large(5),
            16385..=32767 => Slab::Large(6),
            32768..=65536 => Slab::Large(7),
            65537..=131_072 => Slab::Large(8),
            131_073..=262_144 => Slab::Huge(0),
            262_145..=524_288 => Slab::Huge(1),
            524_289..=1_048_576 => Slab::Huge(2),
            1_048_577..=2_097_152 => Slab::Huge(3),
            2_097_153..=4_194_304 => Slab::Huge(4),
            4_194_305..=8_388_608 => Slab::Huge(5),
            8_388_609..=16_777_216 => Slab::Huge(6),
            16_777_217..=33_554_432 => Slab::Huge(7),
            33_554_433..=67_108_864 => Slab::Huge(8),
            _ => Slab::Unsupported,
        }
    }
}

unsafe impl crate::Allocator<'static> for ZoneAllocator {
    /// Allocate a pointer to a block of memory described by `layout`.
    fn allocate(&mut self, layout: Layout) -> Result<NonNull<u8>, AllocationError> {
        match ZoneAllocator::get_slab(layout.size()) {
            Slab::Base(idx) => {
                let (ptr, released_pages) = self.small_slabs[idx].allocate_with_release(layout)?;
                if let Some(pages) = released_pages {
                    GLOBAL_PAGE_POOL.recycle_small_pages(pages, None, true);
                }
                Ok(ptr)
            }
            Slab::Large(idx) => {
                let (ptr, released_pages) = self.big_slabs[idx].allocate_with_release(layout)?;
                if let Some(pages) = released_pages {
                    GLOBAL_PAGE_POOL.recycle_large_pages(pages, None, true)
                }
                Ok(ptr)
            }
            Slab::Huge(idx) => {
                let (ptr, released_pages) = self.huge_slabs[idx].allocate_with_release(layout)?;
                if let Some(pages) = released_pages {
                    GLOBAL_PAGE_POOL.recycle_huge_pages(pages, None, true)
                }
                Ok(ptr)
            }
            Slab::Unsupported => Err(AllocationError::InvalidLayout),
        }
    }

    /// Deallocates a pointer to a block of memory, which was
    /// previously allocated by `allocate`.
    ///
    /// # Arguments
    ///  * `ptr` - Address of the memory location to free.
    ///  * `layout` - Memory layout of the block pointed to by `ptr`.
    fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) -> Result<(), AllocationError> {
        match ZoneAllocator::get_slab(layout.size()) {
            Slab::Base(idx) => self.small_slabs[idx].deallocate(ptr, layout),
            Slab::Large(idx) => self.big_slabs[idx].deallocate(ptr, layout),
            Slab::Huge(idx) => self.huge_slabs[idx].deallocate(ptr, layout),
            Slab::Unsupported => Err(AllocationError::InvalidLayout),
        }
    }

    /// Refills the SCAllocator for a given Layout with an ObjectPage.
    ///
    /// # Safety
    /// ObjectPage needs to be emtpy etc.
    unsafe fn refill(
        &mut self,
        layout: Layout,
        new_page: &'static mut ObjectPage<'static>,
    ) -> Result<(), AllocationError> {
        match ZoneAllocator::get_slab(layout.size()) {
            Slab::Base(idx) => {
                self.small_slabs[idx].refill(new_page);
                Ok(())
            }
            Slab::Large(_idx) => Err(AllocationError::InvalidLayout),
            Slab::Huge(_idx) => Err(AllocationError::InvalidLayout),
            Slab::Unsupported => Err(AllocationError::InvalidLayout),
        }
    }

    /// Refills the SCAllocator for a given Layout with an ObjectPage.
    ///
    /// # Safety
    /// ObjectPage needs to be emtpy etc.
    unsafe fn refill_large(
        &mut self,
        layout: Layout,
        new_page: &'static mut LargeObjectPage<'static>,
    ) -> Result<(), AllocationError> {
        match ZoneAllocator::get_slab(layout.size()) {
            Slab::Base(_idx) => Err(AllocationError::InvalidLayout),
            Slab::Large(idx) => {
                self.big_slabs[idx].refill(new_page);
                Ok(())
            }
            Slab::Huge(_idx) => Err(AllocationError::InvalidLayout),
            Slab::Unsupported => Err(AllocationError::InvalidLayout),
        }
    }

    /// Refills the SCAllocator for a given Layout with an ObjectPage.
    ///
    /// # Safety
    /// ObjectPage needs to be emtpy etc.
    unsafe fn refill_huge(
        &mut self,
        layout: Layout,
        new_page: &'static mut HugeObjectPage<'static>,
    ) -> Result<(), AllocationError> {
        match ZoneAllocator::get_slab(layout.size()) {
            Slab::Base(_idx) => Err(AllocationError::InvalidLayout),
            Slab::Large(_idx) => Err(AllocationError::InvalidLayout),
            Slab::Huge(idx) => {
                self.huge_slabs[idx].refill(new_page);
                Ok(())
            }
            Slab::Unsupported => Err(AllocationError::InvalidLayout),
        }
    }
}

impl Drop for ZoneAllocator {
    fn drop(&mut self) {
        unsafe {
            self.relinquish_pages();
        }
    }
}
impl ZoneAllocator {
    unsafe fn relinquish_pages(&mut self) {
        for slab in self.small_slabs.iter_mut() {
            slab.check_page_assignments();
            let empty_pages = slab.relinquish_empty_pages();
            GLOBAL_PAGE_POOL.recycle_small_pages(empty_pages, None, true);
            let (used_pages, obj_per_page) = slab.relinquish_used_pages();
            GLOBAL_PAGE_POOL.recycle_small_pages(used_pages, Some(obj_per_page), false);
        }

        for slab in self.big_slabs.iter_mut() {
            slab.check_page_assignments();
            let empty_pages = slab.relinquish_empty_pages();
            GLOBAL_PAGE_POOL.recycle_large_pages(empty_pages, None, true);
            let (used_pages, obj_per_page) = slab.relinquish_used_pages();
            GLOBAL_PAGE_POOL.recycle_large_pages(used_pages, Some(obj_per_page), false);
        }

        for slab in self.huge_slabs.iter_mut() {
            slab.check_page_assignments();
            let empty_pages = slab.relinquish_empty_pages();
            GLOBAL_PAGE_POOL.recycle_huge_pages(empty_pages, None, true);
            let (used_pages, obj_per_page) = slab.relinquish_used_pages();
            GLOBAL_PAGE_POOL.recycle_huge_pages(used_pages, Some(obj_per_page), false);
        }
    }
}
