#![allow(dead_code)]
use std::alloc::{AllocError, Allocator, Layout};
use std::ptr;
use std::ptr::NonNull;

struct AlignedAllocator;

const PAGE_SIZE: usize = 4096;

unsafe impl Allocator for AlignedAllocator {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, std::alloc::AllocError> {
        let mut addr = ptr::null_mut();
        let err =
            unsafe { libc::posix_memalign(&mut addr as *mut _ as _, PAGE_SIZE, layout.size()) };
        if err != 0 {
            return Err(AllocError);
        }

        let ptr = NonNull::new(addr).unwrap();
        Ok(NonNull::slice_from_raw_parts(ptr, layout.size()))
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, _layout: Layout) {
        libc::free(ptr.as_ptr() as _);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_usage() {
        let v = Vec::with_capacity_in(opts.msg_size, AlignedAllocator);
        assert!((v.as_ptr() as usize) % PAGE_SIZE == 0);
    }
}
