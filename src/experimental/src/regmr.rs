use std::os::unix::io::RawFd;
use std::ptr;

use nix::fcntl::OFlag;
use nix::sys::mman::{mmap, munmap, shm_open, shm_unlink, MapFlags, ProtFlags};
use nix::sys::stat::Mode;
use nix::unistd::{close, ftruncate};

const SHM_FILE: &'static str = "testpage";

pub struct MR {
    addr: *mut u8,
    len: usize,
    memfd: RawFd,
}

impl Drop for MR {
    fn drop(&mut self) {
        // let _ = unsafe { munmap(self.addr as _, self.len) };
        let _ = close(self.memfd);
        let _ = shm_unlink(SHM_FILE);
    }
}

pub fn reg_mr(addr: *const u8, len: usize) -> Result<MR, nix::Error> {
    let fd = shm_open(
        SHM_FILE,
        OFlag::O_CREAT | OFlag::O_RDWR | OFlag::O_EXCL,
        Mode::S_IRUSR | Mode::S_IWUSR,
    )?;

    ftruncate(fd, len as _)?;

    let old_pa = unsafe {
        mmap(
            ptr::null_mut(),
            len,
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
            MapFlags::MAP_SHARED | MapFlags::MAP_NORESERVE,
            fd,
            0,
        )?
    };

    unsafe {
        // maybe ptr::copy?
        ptr::copy_nonoverlapping(addr, old_pa as _, len);
    }

    unsafe { munmap(old_pa as _, len)? };

    let pa = unsafe {
        mmap(
            addr as _,
            len,
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
            MapFlags::MAP_SHARED | MapFlags::MAP_NORESERVE | MapFlags::MAP_FIXED,
            fd,
            0,
        )?
    };

    assert_eq!(pa, addr as _);

    Ok(MR {
        addr: pa as _,
        len,
        memfd: fd,
    })
}

#[cfg(test)]
mod tests {
    use nix::unistd::lseek;
    use std::thread;
    use std::time::Duration;

    use super::*;

    #[test]
    fn test_reg_mr_aligned() -> Result<(), nix::Error> {
        let len = 4096;
        let text = b"test_reg_mr_aligned";

        let child = unsafe { libc::fork() };
        if child != 0 {
            // parent process
            let mut addr: *mut libc::c_void = ptr::null_mut();
            let err = unsafe { libc::posix_memalign(&mut addr as *mut _, len, len) };
            if err != 0 {
                return Err(nix::Error::from_i32(err));
            }
            let mr = reg_mr(addr as _, len)?;
            unsafe {
                ptr::copy_nonoverlapping(text.as_ptr(), mr.addr as _, text.len());
            }
            thread::sleep(Duration::from_secs(2));
            unsafe {
                libc::free(addr);
            }
        } else {
            // child process
            thread::sleep(Duration::from_secs(1));

            let fd = shm_open(
                SHM_FILE,
                OFlag::O_CREAT | OFlag::O_RDWR,
                Mode::S_IRUSR | Mode::S_IWUSR,
            )?;

            let len = lseek(fd, 0, nix::unistd::Whence::SeekEnd)? as usize;
            assert_eq!(len, 4096);

            let pa = unsafe {
                mmap(
                    ptr::null_mut(),
                    len,
                    ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                    MapFlags::MAP_SHARED | MapFlags::MAP_NORESERVE,
                    fd,
                    0,
                )?
            };

            let data = unsafe { std::slice::from_raw_parts(pa as *const u8, text.len()) };
            assert_eq!(data, text);

            unsafe {
                let _ = munmap(pa, len);
            }
        }
        Ok(())
    }
}
