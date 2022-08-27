/// Directly acquire shared memory from the OS.
use std::alloc::{AllocError, Layout};

use super::ShmAllocator;
use crate::ptr::ShmNonNull;

/// The default memory allocator provided by the operating system.
#[derive(Default)]
pub struct System;

#[cfg(feature = "mrpc")]
mod mrpc {
    use super::*;
    unsafe impl ShmAllocator for System {
        #[inline]
        fn allocate(&self, _layout: Layout) -> Result<ShmNonNull<[u8]>, AllocError> {
            panic!("Should never use System's ShmAllocator when use with mRPC")
        }
        #[inline]
        fn deallocate(&self, _ptr: ShmNonNull<u8>, _layout: Layout) {
            panic!("Should never use System's ShmAllocator when use with mRPC")
        }
    }
}

#[cfg(not(feature = "mrpc"))]
mod notmrpc {
    use super::*;
    use std::fs::File;
    use std::mem;
    use std::os::unix::io::{FromRawFd, RawFd};
    use std::ptr;

    use nix::fcntl::OFlag;
    use nix::sys::mman::shm_open;
    use nix::sys::stat::Mode;
    use nix::unistd::close;

    use mmap::MmapAligned;

    struct Tailroom {
        fd: RawFd,
        mmap: MmapAligned,
    }

    unsafe impl ShmAllocator for System {
        #[inline]
        fn allocate(&self, layout: Layout) -> Result<ShmNonNull<[u8]>, AllocError> {
            // allocate layout
            let fd = match shm_open(
                "unnamed",
                OFlag::O_CREAT | OFlag::O_RDWR | OFlag::O_EXCL,
                Mode::S_IRUSR | Mode::S_IWUSR,
            ) {
                Ok(fd) => fd,
                Err(_e) => return Err(AllocError),
            };

            let memfile = unsafe { File::from_raw_fd(fd) };

            // Allocate extra room for the metadata
            let layout_with_tailroom =
                Layout::from_size_align(layout.size() + mem::size_of::<Tailroom>(), layout.align())
                    .unwrap();

            let mmap = match MmapAligned::map_aligned(&memfile, layout_with_tailroom) {
                Ok(mmap) => mmap.0,
                Err(_e) => return Err(AllocError),
            };

            let ptr = ShmNonNull::slice_from_raw_parts(
                ptr::NonNull::new(mmap.as_mut_ptr()).unwrap(),
                ptr::NonNull::new(mmap.as_mut_ptr()).unwrap(),
                layout.size(),
            );

            // write fd and MmapAligned into the tailroom
            unsafe {
                mmap.as_mut_ptr()
                    .add(layout.size())
                    .cast::<Tailroom>()
                    .write(Tailroom { fd, mmap });
            }

            Ok(ptr)
        }

        #[inline]
        fn deallocate(&self, ptr: ShmNonNull<u8>, layout: Layout) {
            // SAFETY: Memory should only be deallocated once, and the extra tailroom we created
            // should not be overwritten by user (if the user behaves correctly).
            let tailroom = unsafe {
                ptr.as_ptr_app()
                    .add(layout.size())
                    .cast::<Tailroom>()
                    .read()
            };

            // munmap
            drop(tailroom.mmap);
            // close the fd
            let _ = close(tailroom.fd);
        }
    }
}
