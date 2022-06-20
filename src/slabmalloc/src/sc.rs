//! A SCAllocator that can allocate fixed size objects.
use arrayvec::ArrayVec;

use crate::*;

/// A genius(?) const min()
///
/// # What this does
/// * create an array of the two elements you want to choose between
/// * create an arbitrary boolean expression
/// * cast said expresison to a usize
/// * use that value to index into the array created above
///
/// # Source
/// https://stackoverflow.com/questions/53619695/calculating-maximum-value-of-a-set-of-constant-expressions-at-compile-time
#[cfg(feature = "unstable")]
const fn cmin(a: usize, b: usize) -> usize {
    [a, b][(a > b) as usize]
}

/// The boring variant of min (not const).
#[cfg(not(feature = "unstable"))]
fn cmin(a: usize, b: usize) -> usize {
    core::cmp::min(a, b)
}

pub(crate) const RELEASE_BUFFER_SIZE: usize = 32;

/// A slab allocator allocates elements of a fixed size.
///
/// It maintains three internal lists of objects that implement `AllocablePage`
/// from which it can allocate memory.
///
///  * `empty_slabs`: Is a list of pages that the SCAllocator maintains, but
///    has 0 allocations in them, these can be given back to a requestor in case
///    of reclamation.
///  * `slabs`: A list of pages partially allocated and still have room for more.
///  * `full_slabs`: A list of pages that are completely allocated.
///
/// On allocation we allocate memory from `slabs`, however if the list is empty
/// we try to reclaim a page from `empty_slabs` before we return with an out-of-memory
/// error. If a page becomes full after the allocation we move it from `slabs` to
/// `full_slabs`.
///
/// Similarly, on dealloaction we might move a page from `full_slabs` to `slabs`
/// or from `slabs` to `empty_slabs` after we deallocated an object.
///
/// If an allocation returns `OutOfMemory` a client using SCAllocator can refill
/// it using the `refill` function.
pub struct SCAllocator<'a, P: AllocablePage> {
    /// Maximum possible allocation size for this `SCAllocator`.
    pub(crate) size: usize,
    /// Keeps track of succeeded allocations.
    pub(crate) allocation_count: usize,
    /// max objects per page
    pub(crate) obj_per_page: usize,
    /// List of empty ObjectPages (nothing allocated in these).
    pub(crate) empty_slabs: PageList<'a, P>,
    /// List of partially used ObjectPage (some objects allocated but pages are not full).
    pub(crate) slabs: PageList<'a, P>,
    /// List of full ObjectPages (everything allocated in these don't need to search them).
    pub(crate) full_slabs: PageList<'a, P>,

    // a buffer of addrs of the pages (shared regions) to be released
    pub(crate) release_buffer: ArrayVec<&'a mut P, RELEASE_BUFFER_SIZE>,
    // a counter for releasing empty pages to the release buffer
    pub(crate) release_count: usize,
}

/// Creates an instance of a scallocator, we do this in a macro because we
/// re-use the code in const and non-const functions
macro_rules! new_sc_allocator {
    ($size:expr) => {
        SCAllocator {
            size: $size,
            allocation_count: 0,
            obj_per_page: cmin((P::SIZE - 80) / $size, 8 * 64),
            empty_slabs: PageList::new(),
            slabs: PageList::new(),
            full_slabs: PageList::new(),
            release_buffer: ArrayVec::new_const(),
            release_count: 0,
        }
    };
}

impl<'a, P: AllocablePage> SCAllocator<'a, P> {
    const REBALANCE_COUNT: usize = 64;
    // controls the frequency of releasing empty pages
    // performs a page release after RELEASE_COUNT times rebalance
    #[cfg(not(test))]
    const RELEASE_COUNT: usize = 4;
    #[cfg(test)]
    const RELEASE_COUNT: usize = usize::MAX;

    /// Create a new SCAllocator.
    #[cfg(feature = "unstable")]
    pub const fn new(size: usize) -> SCAllocator<'a, P> {
        new_sc_allocator!(size)
    }

    #[cfg(not(feature = "unstable"))]
    pub fn new(size: usize) -> SCAllocator<'a, P> {
        new_sc_allocator!(size)
    }

    /// Returns the maximum supported object size of this allocator.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Add a new ObjectPage.
    fn insert_partial_slab(&mut self, new_head: &'a mut P) {
        self.slabs.insert_front(new_head);
    }

    /// Add page to empty list.
    fn insert_empty(&mut self, new_head: &'a mut P) {
        assert_eq!(
            new_head as *const P as usize % P::SIZE,
            0,
            "Inserted page is not aligned to page-size."
        );
        self.empty_slabs.insert_front(new_head);
    }

    /// Since `dealloc` can not reassign pages without requiring a lock
    /// we check slabs and full slabs periodically as part of `alloc`
    /// and move them to the empty or partially allocated slab lists.
    pub(crate) fn check_page_assignments(&mut self) {
        for slab_page in self.full_slabs.iter_mut() {
            if !slab_page.is_full() {
                // We need to move it from self.full_slabs -> self.slabs
                trace!("move {:p} full -> partial", slab_page);
                self.move_full_to_partial(slab_page);
            }
        }

        for slab_page in self.slabs.iter_mut() {
            if slab_page.is_empty(self.obj_per_page) {
                // We need to move it from self.slabs -> self.empty_slabs
                trace!("move {:p} partial -> empty", slab_page);
                self.move_to_empty(slab_page);
            }
        }
    }

    pub(crate) fn release_empty_pages(&mut self) {
        let mut cap = self.release_buffer.remaining_capacity();
        while cap > 0 && self.empty_slabs.head.is_some() {
            let slab = self.empty_slabs.pop().unwrap();
            // NOTE(wyj): use push unchecked.
            self.release_buffer.push(slab);
            cap -= 1;
        }
    }

    /// Move a page from `slabs` to `empty_slabs`.
    fn move_to_empty(&mut self, page: &'a mut P) {
        let page_ptr = page as *const P;

        debug_assert!(self.slabs.contains(page_ptr));
        debug_assert!(
            !self.empty_slabs.contains(page_ptr),
            "Page {:p} already in emtpy_slabs",
            page_ptr
        );

        self.slabs.remove_from_list(page);
        self.empty_slabs.insert_front(page);

        debug_assert!(!self.slabs.contains(page_ptr));
        debug_assert!(self.empty_slabs.contains(page_ptr));
    }

    /// Move a page from `full_slabs` to `slab`.
    fn move_partial_to_full(&mut self, page: &'a mut P) {
        let page_ptr = page as *const P;

        debug_assert!(self.slabs.contains(page_ptr));
        debug_assert!(!self.full_slabs.contains(page_ptr));

        self.slabs.remove_from_list(page);
        self.full_slabs.insert_front(page);

        debug_assert!(!self.slabs.contains(page_ptr));
        debug_assert!(self.full_slabs.contains(page_ptr));
    }

    /// Move a page from `full_slabs` to `slab`.
    fn move_full_to_partial(&mut self, page: &'a mut P) {
        let page_ptr = page as *const P;

        debug_assert!(!self.slabs.contains(page_ptr));
        debug_assert!(self.full_slabs.contains(page_ptr));

        self.full_slabs.remove_from_list(page);
        self.slabs.insert_front(page);

        debug_assert!(self.slabs.contains(page_ptr));
        debug_assert!(!self.full_slabs.contains(page_ptr));
    }

    /// Tries to allocate a block of memory with respect to the `layout`.
    /// Searches within already allocated slab pages, if no suitable spot is found
    /// will try to use a page from the empty page list.
    ///
    /// # Arguments
    ///  * `sc_layout`: This is not the original layout but adjusted for the
    ///     SCAllocator size (>= original).
    fn try_allocate_from_pagelist(&mut self, sc_layout: Layout) -> (*mut u8, bool) {
        // TODO: Do we really need to check multiple slab pages (due to alignment)
        // If not we can get away with a singly-linked list and have 8 more bytes
        // for the bitfield in an ObjectPage.

        for slab_page in self.slabs.iter_mut() {
            let ptr = slab_page.allocate(sc_layout);
            if !ptr.is_null() {
                if slab_page.is_full() {
                    trace!("move {:p} partial -> full", slab_page);
                    self.move_partial_to_full(slab_page);
                }
                self.allocation_count += 1;
                return (ptr, false);
            } else {
                continue;
            }
        }

        // Periodically rebalance page-lists (since dealloc can't do it for us)
        if self.allocation_count % SCAllocator::<P>::REBALANCE_COUNT == 0 {
            self.check_page_assignments();
            self.allocation_count = 0;
            self.release_count += 1;
            if self.release_count > SCAllocator::<P>::RELEASE_COUNT {
                self.release_empty_pages();
                self.release_count = 0;
                return (ptr::null_mut(), true);
            }
        }
        (ptr::null_mut(), false)
    }

    /// Refill the SCAllocator
    ///
    /// # Safety
    /// ObjectPage needs to be empty etc.
    pub unsafe fn refill(&mut self, page: &'a mut P) {
        page.bitfield_mut().initialize(self.size, P::SIZE - 80);
        *page.prev() = Rawlink::none();
        *page.next() = Rawlink::none();
        trace!("adding page to SCAllocator {:p}", page);
        self.insert_empty(page);
    }

    pub(crate) unsafe fn relinquish_empty_pages(&mut self) -> alloc::vec::Vec<&'a mut P> {
        let mut relinquished = alloc::vec::Vec::new();
        while let Some(slab) = self.empty_slabs.pop() {
            relinquished.push(slab);
        }
        relinquished
    }

    //  relinquish used pages, together with obj_per_page for empty check
    pub(crate) unsafe fn relinquish_used_pages(&mut self) -> alloc::vec::Vec<(&'a mut P, usize)> {
        let mut relinquished = alloc::vec::Vec::new();
        while let Some(slab) = self.slabs.pop() {
            relinquished.push((slab, self.obj_per_page));
        }
        while let Some(slab) = self.full_slabs.pop() {
            relinquished.push((slab, self.obj_per_page));
        }
        relinquished
    }

    /// Allocates a block of memory descriped by `layout`.
    ///
    /// Returns a pointer to a valid region of memory or an
    /// AllocationError.
    ///
    /// The function may also move around pages between lists
    /// (empty -> partial or partial -> full).
    pub fn allocate(&mut self, layout: Layout) -> Result<NonNull<u8>, AllocationError> {
        trace!(
            "SCAllocator({}) is trying to allocate {:?}",
            self.size,
            layout
        );
        assert!(layout.size() <= self.size);
        assert!(self.size <= (P::SIZE - CACHE_LINE_SIZE));
        let new_layout = unsafe { Layout::from_size_align_unchecked(self.size, layout.align()) };
        assert!(new_layout.size() >= layout.size());

        let ptr = {
            // Try to allocate from partial slabs,
            // if we fail check if we have empty pages and allocate from there
            let (ptr, _) = self.try_allocate_from_pagelist(new_layout);
            if ptr.is_null() && self.empty_slabs.head.is_some() {
                // Re-try allocation in empty page
                let empty_page = self.empty_slabs.pop().expect("We checked head.is_some()");
                debug_assert!(!self.empty_slabs.contains(empty_page));

                let ptr = empty_page.allocate(layout);
                debug_assert!(!ptr.is_null(), "Allocation must have succeeded here.");

                trace!(
                    "move {:p} empty -> partial empty count {}",
                    empty_page,
                    self.empty_slabs.elements
                );
                // Move empty page to partial pages
                self.insert_partial_slab(empty_page);
                ptr
            } else {
                ptr
            }
        };

        let res = NonNull::new(ptr).ok_or(AllocationError::OutOfMemory);

        if !ptr.is_null() {
            trace!(
                "SCAllocator({}) allocated ptr=0x{:x}",
                self.size,
                ptr as usize
            );
        }

        res
    }

    /// Allocate with the opportunity to recycle released empty slabs
    pub fn allocate_with_release(
        &mut self,
        layout: Layout,
    ) -> Result<
        (
            NonNull<u8>,
            Option<arrayvec::Drain<&'a mut P, RELEASE_BUFFER_SIZE>>,
        ),
        AllocationError,
    > {
        trace!(
            "SCAllocator({}) is trying to allocate {:?}",
            self.size,
            layout
        );
        assert!(layout.size() <= self.size);
        assert!(self.size <= (P::SIZE - CACHE_LINE_SIZE));
        let new_layout = unsafe { Layout::from_size_align_unchecked(self.size, layout.align()) };
        assert!(new_layout.size() >= layout.size());

        let (ptr, release_flag) = {
            // Try to allocate from partial slabs,
            // if we fail check if we have empty pages and allocate from there
            let (ptr, release_flag) = self.try_allocate_from_pagelist(new_layout);
            if ptr.is_null() && self.empty_slabs.head.is_some() {
                // Re-try allocation in empty page
                let empty_page = self.empty_slabs.pop().expect("We checked head.is_some()");
                debug_assert!(!self.empty_slabs.contains(empty_page));

                let ptr = empty_page.allocate(layout);
                debug_assert!(!ptr.is_null(), "Allocation must have succeeded here.");

                trace!(
                    "move {:p} empty -> partial empty count {}",
                    empty_page,
                    self.empty_slabs.elements
                );
                // Move empty page to partial pages
                self.insert_partial_slab(empty_page);
                (ptr, release_flag)
            } else {
                (ptr, release_flag)
            }
        };

        let release_slabs = if release_flag {
            Some(self.release_buffer.drain(..))
        } else {
            None
        };

        let res = NonNull::new(ptr)
            .ok_or(AllocationError::OutOfMemory)
            .map(|ptr| (ptr, release_slabs));

        if !ptr.is_null() {
            trace!(
                "SCAllocator({}) allocated ptr=0x{:x}",
                self.size,
                ptr as usize
            );
        }

        res
    }

    /// Deallocates a previously allocated `ptr` described by `Layout`.
    ///
    /// May return an error in case an invalid `layout` is provided.
    /// The function may also move internal slab pages between lists partial -> empty
    /// or full -> partial lists.
    pub fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) -> Result<(), AllocationError> {
        assert!(layout.size() <= self.size);
        assert!(self.size <= (P::SIZE - CACHE_LINE_SIZE));
        trace!(
            "SCAllocator({}) is trying to deallocate ptr = {:p} layout={:?} P.size= {}",
            self.size,
            ptr,
            layout,
            P::SIZE
        );

        let page = (ptr.as_ptr() as usize) & !(P::SIZE - 1) as usize;

        // Figure out which page we are on and construct a reference to it
        // TODO: The linked list will have another &mut reference
        let slab_page = unsafe { mem::transmute::<VAddr, &'a mut P>(page) };
        let new_layout = unsafe { Layout::from_size_align_unchecked(self.size, layout.align()) };

        let ret = slab_page.deallocate(ptr, new_layout);
        debug_assert!(ret.is_ok(), "Slab page deallocate won't fail at the moment");
        ret
    }
}
