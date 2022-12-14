//! This crate implements more functionalities to memmap2.
//! E.g., MAP_FIXED, map aligned, and HUGETLB.
use std::fs;
use std::io;
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
use std::os::unix::io::{FromRawFd, RawFd};
use std::ptr;
use std::slice;

use nix::sys::mman::{mmap, munmap, MapFlags, ProtFlags};

/// It is the responsibility of the application to know which sizes are supported on
/// the running system.  See mmap(2) man page for details.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MapHugePage {
    Huge64KB = libc::MAP_HUGE_64KB,
    Huge512K = libc::MAP_HUGE_512KB,
    Huge1MB = libc::MAP_HUGE_1MB,
    Huge8MB = libc::MAP_HUGE_8MB,
    Huge16MB = libc::MAP_HUGE_16MB,
    Huge32MB = libc::MAP_HUGE_32MB,
    Huge256MB = libc::MAP_HUGE_256MB,
    Huge512MB = libc::MAP_HUGE_512MB,
    Huge1GB = libc::MAP_HUGE_1GB,
    Huge2GB = libc::MAP_HUGE_2GB,
    Huge16GB = libc::MAP_HUGE_16GB,
}

#[derive(Debug, Clone)]
pub struct MmapOptions {
    /// The target address if MAP_FIXED_NOREPLACE
    fixed_noreplace: Option<usize>,
    /// Mapped length. This option is mandatory for anonymous memory maps.
    /// For file-backed memory maps, the length will default to the file length.
    map_len: Option<usize>,
    /// Memory protection flags
    prot_flags: ProtFlags,
    /// Additional parameter for mmap
    map_flags: MapFlags,
    /// The file descriptor to the memory object (e.g., a file)
    fd: Option<RawFd>,
    /// The map starts at `file_off` offset in the memory object. `file_off` must be
    /// a multiple of the page size as returned by sysconf(_SC_PAGE_SIZE).
    file_off: i64,
}

impl Default for MmapOptions {
    fn default() -> Self {
        Self {
            fixed_noreplace: None,
            map_len: None,
            prot_flags: ProtFlags::empty(),
            map_flags: MapFlags::empty(),
            fd: None,
            file_off: 0,
        }
    }
}

impl MmapOptions {
    pub fn new() -> Self {
        MmapOptions::default()
    }

    pub fn offset(&mut self, offset: u64) -> &mut Self {
        self.file_off = i64::try_from(offset).expect("invalid arguments");
        self
    }

    pub fn len(&mut self, len: usize) -> &mut Self {
        self.map_len = Some(len);
        self
    }

    pub fn fixed_noreplace(&mut self, target_addr: usize) -> &mut Self {
        self.fixed_noreplace = Some(target_addr);
        self
    }

    pub fn set_fd(&mut self, fd: RawFd) -> &mut Self {
        self.fd = Some(fd);
        self
    }

    /* ==================== Mmap Flags ==================== */
    pub fn anon(&mut self, enable: bool) -> &mut Self {
        self.map_flags.set(MapFlags::MAP_ANONYMOUS, enable);
        self
    }

    pub fn huge_page(&mut self, huge_page: MapHugePage) -> &mut Self {
        self.map_flags.set(MapFlags::MAP_HUGETLB, true);
        self.map_flags
            .set(MapFlags::from_bits(huge_page as _).unwrap(), true);
        self
    }

    pub fn private(&mut self, enable: bool) -> &mut Self {
        self.map_flags.set(MapFlags::MAP_PRIVATE, enable);
        self
    }

    pub fn shared(&mut self, enable: bool) -> &mut Self {
        self.map_flags.set(MapFlags::MAP_SHARED, enable);
        self
    }

    pub fn populate(&mut self, enable: bool) -> &mut Self {
        self.map_flags.set(MapFlags::MAP_POPULATE, enable);
        self
    }

    pub fn reserve(&mut self, enable: bool) -> &mut Self {
        self.map_flags.set(MapFlags::MAP_NORESERVE, !enable);
        self
    }

    /* ==================== Protection Flags ==================== */
    pub fn read(&mut self, enable: bool) -> &mut Self {
        self.prot_flags.set(ProtFlags::PROT_READ, enable);
        self
    }

    pub fn write(&mut self, enable: bool) -> &mut Self {
        self.prot_flags.set(ProtFlags::PROT_WRITE, enable);
        self
    }

    pub fn exec(&mut self, enable: bool) -> &mut Self {
        self.prot_flags.set(ProtFlags::PROT_EXEC, enable);
        self
    }

    pub fn mmap(&self) -> io::Result<Mmap> {
        let target_addr = self
            .fixed_noreplace
            .map_or(ptr::null_mut(), |x| x as *mut libc::c_void);
        let map_len = match self.map_len {
            None => {
                match self.fd {
                    Some(fd) => {
                        // SAFETY: This is fine because we do not drop the file.
                        let file = ManuallyDrop::new(unsafe { fs::File::from_raw_fd(fd) });
                        file.metadata()?.len() as usize
                    }
                    None => {
                        // The mmap will return an Error later
                        0
                    }
                }
            }
            Some(len) => len,
        };
        let fd = self.fd.unwrap_or(-1);
        let ptr = unsafe { mmap(target_addr, map_len, self.prot_flags, self.map_flags, fd, 0)? };

        Ok(Mmap { ptr, len: map_len })
    }
}

#[repr(C)]
pub struct Mmap {
    ptr: *mut libc::c_void,
    len: usize,
}

impl Drop for Mmap {
    fn drop(&mut self) {
        unsafe {
            munmap(self.ptr, self.len).unwrap_or_else(|e| eprintln!("failed to munmap: {}", e))
        };
    }
}

impl Mmap {
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
unsafe impl Sync for Mmap {}
unsafe impl Send for Mmap {}

impl Deref for Mmap {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.as_ptr(), self.len()) }
    }
}

impl AsRef<[u8]> for Mmap {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

impl DerefMut for Mmap {
    #[inline]
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.as_mut_ptr(), self.len()) }
    }
}

use std::fmt;
impl fmt::Debug for Mmap {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.debug_struct("Mmap")
            .field("ptr", &self.as_ptr())
            .field("len", &self.len())
            .finish()
    }
}
