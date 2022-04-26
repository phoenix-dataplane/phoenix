use nix::sys::mman::{mmap, munmap, MapFlags, ProtFlags};
use std::fs;
use std::io;
use std::ops::{Deref, DerefMut};
use std::os::unix::io::AsRawFd;
use std::slice;

pub struct MmapFixed {
    ptr: *mut libc::c_void,
    len: usize,
}

impl Drop for MmapFixed {
    fn drop(&mut self) {
        unsafe {
            munmap(self.ptr, self.len).unwrap_or_else(|e| eprintln!("failed to munmap: {}", e))
        };
    }
}

impl MmapFixed {
    pub fn new(
        target_addr: usize,
        map_len: usize,
        file_off: i64,
        memfile: &fs::File,
    ) -> io::Result<Self> {
        let len = memfile.metadata()?.len() as usize;
        assert!(len >= map_len);
        // TODO(cjr): use MAP_FIXED_NOREPLACE
        let ptr = unsafe {
            mmap(
                target_addr as *mut libc::c_void,
                map_len,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED | MapFlags::MAP_NORESERVE | MapFlags::MAP_FIXED,
                memfile.as_raw_fd(),
                file_off,
            )?
        };
        assert_eq!(ptr as usize, target_addr);
        Ok(Self { ptr, len: map_len })
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
unsafe impl Sync for MmapFixed {}
unsafe impl Send for MmapFixed {}

impl Deref for MmapFixed {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.as_ptr(), self.len()) }
    }
}

impl AsRef<[u8]> for MmapFixed {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

impl DerefMut for MmapFixed {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.as_mut_ptr(), self.len()) }
    }
}

use std::fmt;
impl fmt::Debug for MmapFixed {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("MmapFixed")
            .field("ptr", &self.as_ptr())
            .field("len", &self.len())
            .finish()
    }
}
