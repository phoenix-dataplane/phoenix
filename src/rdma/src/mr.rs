//! Shared memory region.
#![cfg(feature = "koala")]
use std::io;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use memfd::{Memfd, MemfdOptions};
use memmap2::{MmapMut, MmapOptions};
use thiserror::Error;

use crate::{ffi, ibv, rdmacm};

use interface::{AccessFlags, RemoteKey};
use interface::{AsHandle, Handle};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Memfd: {0}.")]
    Memfd(#[from] memfd::Error),
    #[error("IO: {0}.")]
    Io(#[from] io::Error),
}

pub struct MemoryRegion {
    mr: *mut ffi::ibv_mr,
    // mmap: MmapMut,
    mmap: MmapAligned,
    memfd: Memfd,
    file_off: usize,
}

unsafe impl Send for MemoryRegion {}
unsafe impl Sync for MemoryRegion {}

impl Deref for MemoryRegion {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.mmap[..]
    }
}

impl DerefMut for MemoryRegion {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.mmap[..]
    }
}

impl Drop for MemoryRegion {
    fn drop(&mut self) {
        let errno = unsafe { ffi::ibv_dereg_mr(self.mr) };
        if errno != 0 {
            let e = io::Error::from_raw_os_error(errno);
            panic!("{}", e);
        }
    }
}

impl AsHandle for MemoryRegion {
    #[inline]
    fn as_handle(&self) -> Handle {
        assert!(!self.mr.is_null());
        Handle(unsafe { &*self.mr }.handle)
    }
}

impl MemoryRegion {
    pub fn new<'ctx>(
        pd: &'ctx ibv::ProtectionDomain<'ctx>,
        nbytes: usize,
        access: AccessFlags,
    ) -> Result<Self, Error> {
        let opts = MemfdOptions::default()
            .allow_sealing(true)
            .close_on_exec(false);
        let name = format!("shared-mr-{}", nbytes);
        let memfd = opts.create(name)?;
        memfd.as_file().set_len(nbytes as u64)?;

        // let mut mmap = unsafe { MmapOptions::new().map_mut(memfd.as_file()) }?;
        // assert!(mmap.as_ptr() as usize % page_size() == 0);
        let (mmap, file_off) = MmapAligned::map_aligned(memfd.as_file())?;

        let mr = unsafe {
            ffi::ibv_reg_mr(
                pd.pd,
                mmap.as_mut_ptr() as *mut _,
                nbytes as _,
                ibv::AccessFlags::from(access).0 .0 as _,
            )
        };

        if mr.is_null() {
            panic!("{:?}", Error::Io(io::Error::last_os_error()));
            Err(Error::Io(io::Error::last_os_error()))
        } else {
            Ok(Self { mr, mmap, memfd, file_off })
        }
    }

    #[inline]
    pub fn memfd(&self) -> &Memfd {
        &self.memfd
    }

    #[inline]
    pub fn mr(&self) -> *mut ffi::ibv_mr {
        self.mr
    }

    #[inline]
    pub fn pd<'a>(&self) -> &ibv::ProtectionDomain<'a> {
        assert!(!self.mr.is_null());
        unsafe { (&(&*self.mr).pd).as_ref() }
    }

    #[inline]
    pub fn rkey(&self) -> RemoteKey {
        assert!(!self.mr.is_null());
        let mr = unsafe { &*self.mr };
        RemoteKey {
            rkey: mr.rkey,
            addr: mr.addr as u64,
        }
    }

    #[inline]
    pub fn file_off(&self) -> usize {
        self.file_off
    }
}

impl<'a, A> From<A> for rdmacm::MemoryRegion<'a>
where
    A: AsRef<MemoryRegion> + 'a,
{
    fn from(a: A) -> rdmacm::MemoryRegion<'a> {
        rdmacm::MemoryRegion(a.as_ref().mr, PhantomData)
    }
}

use nix::sys::mman::{mmap, munmap, MapFlags, ProtFlags};
use std::fs;
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
        let align = if len < 2097152 { 4096 } else { 2097152 };
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
        Ok((Self {
            ptr: aligned_ptr,
            len,
        }, head_len))
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

const fn huge_page_size() -> usize {
    2097152
}

fn page_size() -> usize {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static PAGE_SIZE: AtomicUsize = AtomicUsize::new(0);

    match PAGE_SIZE.load(Ordering::Relaxed) {
        0 => {
            let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize };

            PAGE_SIZE.store(page_size, Ordering::Relaxed);

            page_size
        }
        page_size => page_size,
    }
}
