//! Shared memory allocation APIs.
use std::alloc::{AllocError, Layout};
use std::ptr;

use crate::ptr::ShmNonNull;

pub mod system;
pub use system::System;

pub unsafe trait ShmAllocator {
    fn allocate(&self, layout: Layout) -> Result<ShmNonNull<[u8]>, AllocError>;

    fn deallocate(&self, ptr: ShmNonNull<u8>, layout: Layout);

    fn allocate_zeroed(&self, layout: Layout) -> Result<ShmNonNull<[u8]>, AllocError> {
        let ptr = self.allocate(layout)?;
        // SAFETY: `alloc` returns a valid memory block
        unsafe { ptr.as_mut_ptr_app().write_bytes(0, ptr.len()) }
        Ok(ptr)
    }

    unsafe fn grow(
        &self,
        ptr: ShmNonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<ShmNonNull<[u8]>, AllocError> {
        debug_assert!(
            new_layout.size() >= old_layout.size(),
            "`new_layout.size()` must be greater than or equal to `old_layout.size()`"
        );

        let new_ptr = self.allocate(new_layout)?;

        // SAFETY: because `new_layout.size()` must be greater than or equal to
        // `old_layout.size()`, both the old and new memory allocation are valid for reads and
        // writes for `old_layout.size()` bytes. Also, because the old allocation wasn't yet
        // deallocated, it cannot overlap `new_ptr`. Thus, the call to `copy_nonoverlapping` is
        // safe. The safety contract for `dealloc` must be upheld by the caller.
        ptr::copy_nonoverlapping(
            ptr.as_ptr_app(),
            new_ptr.as_mut_ptr_app(),
            old_layout.size(),
        );
        self.deallocate(ptr, old_layout);

        Ok(new_ptr)
    }

    unsafe fn grow_zeroed(
        &self,
        ptr: ShmNonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<ShmNonNull<[u8]>, AllocError> {
        debug_assert!(
            new_layout.size() >= old_layout.size(),
            "`new_layout.size()` must be greater than or equal to `old_layout.size()`"
        );

        let new_ptr = self.allocate_zeroed(new_layout)?;

        // SAFETY: because `new_layout.size()` must be greater than or equal to
        // `old_layout.size()`, both the old and new memory allocation are valid for reads and
        // writes for `old_layout.size()` bytes. Also, because the old allocation wasn't yet
        // deallocated, it cannot overlap `new_ptr`. Thus, the call to `copy_nonoverlapping` is
        // safe. The safety contract for `dealloc` must be upheld by the caller.
        ptr::copy_nonoverlapping(
            ptr.as_ptr_app(),
            new_ptr.as_mut_ptr_app(),
            old_layout.size(),
        );
        self.deallocate(ptr, old_layout);

        Ok(new_ptr)
    }

    unsafe fn shrink(
        &self,
        ptr: ShmNonNull<u8>,
        old_layout: Layout,
        new_layout: Layout,
    ) -> Result<ShmNonNull<[u8]>, AllocError> {
        debug_assert!(
            new_layout.size() <= old_layout.size(),
            "`new_layout.size()` must be smaller than or equal to `old_layout.size()`"
        );

        let new_ptr = self.allocate(new_layout)?;

        // SAFETY: because `new_layout.size()` must be lower than or equal to
        // `old_layout.size()`, both the old and new memory allocation are valid for reads and
        // writes for `new_layout.size()` bytes. Also, because the old allocation wasn't yet
        // deallocated, it cannot overlap `new_ptr`. Thus, the call to `copy_nonoverlapping` is
        // safe. The safety contract for `dealloc` must be upheld by the caller.
        ptr::copy_nonoverlapping(
            ptr.as_ptr_app(),
            new_ptr.as_mut_ptr_app(),
            new_layout.size(),
        );
        self.deallocate(ptr, old_layout);

        Ok(new_ptr)
    }

    /// Creates a "by reference" adapter for this instance of `Allocator`.
    ///
    /// The returned adapter also implements `Allocator` and will simply borrow this.
    #[inline(always)]
    fn by_ref(&self) -> &Self
    where
        Self: Sized,
    {
        self
    }
}
