use nix::sys::mman::{mmap, munmap, MapFlags, ProtFlags};
use std::fs;
use std::io;
use std::ops::{Deref, DerefMut};
use std::os::unix::io::AsRawFd;
use std::ptr;
use std::slice;

pub struct MmapAligned {
    ptr: *mut libc::c_void,
    len: usize,
}

impl Drop for MmapAligned {
    fn drop(&mut self) {
        unsafe {
            munmap(self.ptr, self.len).unwrap_or_else(|e| eprintln!("failed to munmap: {}", e))
        };
    }
}

impl MmapAligned {
    fn map_aligned(memfile: &fs::File) -> io::Result<(Self, usize)> {
        let len = memfile.metadata()?.len() as usize;
        assert_ne!(len, 0);
        let align = len
            .checked_next_power_of_two()
            .expect("next_power_of_two: {len}");
        assert!(len % align == 0, "len {} vs align {}", len, align);

        let mapped_len = align + len;
        memfile.set_len(mapped_len as u64)?;
        let ptr = unsafe {
            mmap(
                ptr::null_mut(),
                mapped_len,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED | MapFlags::MAP_NORESERVE,
                memfile.as_raw_fd(),
                0,
            )?
        };
        let addr = ptr as usize;
        // cut off the extra head
        let head_len = align - addr % align;
        unsafe { munmap(ptr, head_len).unwrap_or_else(|e| eprintln!("failed to munmap: {}", e)) };
        // cut off the extra tail if any
        let tail_len = align - head_len;
        if tail_len > 0 {
            let tail_addr = addr + mapped_len - tail_len;
            assert!(tail_addr % align == 0, "tail_addr: {:#0x?}", tail_addr);
            unsafe {
                munmap(tail_addr as *mut libc::c_void, tail_len)
                    .unwrap_or_else(|e| eprintln!("failed to munmap: {}", e))
            };
        }

        let aligned_ptr = ptr.cast::<u8>().wrapping_add(head_len).cast();
        log::debug!(
            "ptr: {:0x?}, align: {}, len: {}, mapped_len: {}, head_len: {}, tail_len: {}, aligned_ptr: {:0x?}",
            ptr,
            align,
            len,
            mapped_len,
            head_len,
            tail_len,
            aligned_ptr,
        );

        assert!(aligned_ptr as usize % align == 0);
        Ok((
            Self {
                ptr: aligned_ptr,
                len,
            },
            head_len,
        ))
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns a raw pointer to the memory mapped file.
    ///
    /// Before dereferencing this pointer, you have to make sure that the file has not been
    /// truncated since the memory map was created.
    /// Avoiding this will not introduce memory safety issues in Rust terms,
    /// but will cause SIGBUS (or equivalent) signal.
    #[inline]
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr as *const u8
    }

    /// Returns an unsafe mutable pointer to the memory mapped file.
    ///
    /// Before dereferencing this pointer, you have to make sure that the file has not been
    /// truncated since the memory map was created.
    /// Avoiding this will not introduce memory safety issues in Rust terms,
    /// but will cause SIGBUS (or equivalent) signal.
    #[inline]
    pub fn as_mut_ptr(&self) -> *mut u8 {
        self.ptr as *mut u8
    }
}

// Why this is safe?
unsafe impl Sync for MmapAligned {}
unsafe impl Send for MmapAligned {}

impl Deref for MmapAligned {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.as_ptr(), self.len()) }
    }
}

impl AsRef<[u8]> for MmapAligned {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

impl DerefMut for MmapAligned {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.as_mut_ptr(), self.len()) }
    }
}

use std::fmt;
impl fmt::Debug for MmapAligned {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("MmapAligned")
            .field("ptr", &self.as_ptr())
            .field("len", &self.len())
            .finish()
    }
}
