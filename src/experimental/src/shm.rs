use std::os::unix::io::RawFd;
use std::ptr::{self, NonNull};

use nix::fcntl::OFlag;
use nix::sys::mman::{mmap, shm_open, MapFlags, ProtFlags};
use nix::sys::stat::Mode;
use nix::unistd::{close, ftruncate};
use nix::NixPath;

pub struct SharedMemory {
    ptr: NonNull<u8>,
    len: usize,
    fd: Option<RawFd>,
}

impl Drop for SharedMemory {
    fn drop(&mut self) {
        if let Some(fd) = self.fd {
            let _ = close(fd);
        }
    }
}

impl SharedMemory {
    pub fn new<P: ?Sized + NixPath>(name: &P, len: usize) -> nix::Result<SharedMemory> {
        let fd = shm_open(
            name,
            OFlag::O_CREAT | OFlag::O_RDWR | OFlag::O_EXCL,
            Mode::S_IRUSR | Mode::S_IWUSR,
        )?;

        ftruncate(fd, len as _)?;

        let ptr = unsafe {
            mmap(
                ptr::null_mut(),
                len,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED,
                fd,
                0,
            )?
        };

        Ok(SharedMemory {
            ptr: NonNull::new(ptr as *mut _).unwrap(),
            len,
            fd: Some(fd),
        })
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn as_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }
}
