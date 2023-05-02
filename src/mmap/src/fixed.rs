use std::fs;
use std::io;
use std::ops::{Deref, DerefMut};
use std::os::unix::io::AsRawFd;
use std::slice;

const PRE_POPULATE_THRESHOLD: usize = 8 * 4096;

pub struct MmapFixed {
    ptr: *mut libc::c_void,
    len: usize,
}

impl Drop for MmapFixed {
    fn drop(&mut self) {
        let ret = unsafe { libc::munmap(self.ptr, self.len) };
        if ret == -1 {
            eprintln!(
                "failed to munmap: {:?} because: {}",
                self,
                io::Error::last_os_error()
            );
        }
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

        // let hugetlb = if map_len & 0x1fffff == 0 && target_addr & 0x1fffff == 0 && len & 0x1fffff == 0 {
        //     libc::MAP_HUGETLB | libc::MAP_HUGE_2MB
        // } else {
        //     0
        // };
        let hugetlb = 0;

        let mut flags =
            libc::MAP_SHARED | libc::MAP_NORESERVE | libc::MAP_FIXED_NOREPLACE | hugetlb;

        // Pre-populate if the map size is only a few pages.
        if map_len <= PRE_POPULATE_THRESHOLD || hugetlb != 0 {
            flags |= libc::MAP_POPULATE;
        }

        let ptr = unsafe {
            libc::mmap(
                target_addr as *mut libc::c_void,
                map_len,
                libc::PROT_READ | libc::PROT_WRITE,
                flags,
                memfile.as_raw_fd(),
                file_off,
            )
        };

        if ptr == libc::MAP_FAILED {
            Err(io::Error::last_os_error())
        } else {
            assert_eq!(ptr as usize, target_addr);
            Ok(Self { ptr, len: map_len })
        }
    }

    /// Returns the number of bytes of this memory segment, also referred to as its 'length'.
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

    // TODO(cjr): This is problematic.
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

// TODO(cjr): This is also problematic.
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
