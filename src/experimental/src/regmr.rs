use std::io::{IoSlice, IoSliceMut};
use std::os::unix::io::RawFd;
use std::os::unix::net::{SocketAncillary, AncillaryData ,UnixDatagram};
use std::path::Path;
use std::ptr;

use nix::fcntl::OFlag;
use nix::sys::mman::{mmap, munmap, shm_open, shm_unlink, MapFlags, ProtFlags};
use nix::sys::stat::Mode;
use nix::unistd::{close, ftruncate, sysconf, SysconfVar};

const SHM_FILE: &'static str = "testpage";
const SHM_FILE2: &'static str = "testpage2";

pub struct MR {
    addr: *mut u8,
    len: usize,
    memfd: RawFd,
    shm_file: String,
}

impl Drop for MR {
    fn drop(&mut self) {
        // let _ = unsafe { munmap(self.addr as _, self.len) };
        let _ = close(self.memfd);
        let _ = shm_unlink(self.shm_file.as_str());
    }
}

fn get_page_size() -> usize {
    sysconf(SysconfVar::PAGE_SIZE).unwrap().unwrap() as _
}

pub fn send_fd<P: AsRef<Path>>(sock_path: P, fd: RawFd) -> Result<(), std::io::Error> {
    let sock = UnixDatagram::unbound()?;
    sock.connect(sock_path)?;

    let mut ancillary_buffer = [0; 128];
    let mut ancillary = SocketAncillary::new(&mut ancillary_buffer[..]);
    ancillary.add_fds(&[fd][..]);

    println!("sending fd: {}", fd);

    let bufs = &mut[][..];
    sock.send_vectored_with_ancillary(bufs, &mut ancillary)?;
    Ok(())
}

pub fn recv_fd<P: AsRef<Path>>(sock_path: P) -> Result<RawFd, std::io::Error> {
    let sock = UnixDatagram::bind(&sock_path)?;
    let bufs = &mut[][..];
    let mut fds = Vec::new();
    let mut ancillary_buffer = [0; 128];
    let mut ancillary = SocketAncillary::new(&mut ancillary_buffer[..]);
    let (size, truncated) = sock.recv_vectored_with_ancillary(bufs, &mut ancillary)?;
    println!("received {}", size);
    assert!(!truncated);
    for ancillary_result in ancillary.messages() {
        if let AncillaryData::ScmRights(scm_rights) = ancillary_result.unwrap() {
            for fd in scm_rights {
                println!("receive file descriptor: {}", fd);
                fds.push(fd);
            }
        }
    }
    assert_eq!(fds.len(), 1);

    std::fs::remove_file(sock_path)?;
    Ok(fds[0])
}

pub fn reg_mr(addr: *const u8, len: usize, shm_file: &str) -> Result<MR, nix::Error> {
    let fd = shm_open(
        shm_file,
        OFlag::O_CREAT | OFlag::O_RDWR | OFlag::O_EXCL,
        Mode::S_IRUSR | Mode::S_IWUSR,
    )?;

    let page_size = get_page_size();
    let aligned_end = (addr as usize + len + page_size - 1) / page_size * page_size;
    let aligned_begin = addr as usize - addr as usize % page_size;
    let aligned_len = aligned_end - aligned_begin;

    ftruncate(fd, aligned_len as _)?;

    let old_pa = unsafe {
        mmap(
            ptr::null_mut(),
            aligned_len,
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
            MapFlags::MAP_SHARED | MapFlags::MAP_NORESERVE,
            fd,
            0,
        )?
    };

    unsafe {
        // maybe ptr::copy?
        // copy the whole pages, other control block of memory allocator will be zeroed
        ptr::copy(aligned_begin as _, old_pa, aligned_len);
    }

    unsafe { munmap(old_pa as _, aligned_len)? };

    let pa = unsafe {
        mmap(
            aligned_begin as *mut libc::c_void,
            aligned_len,
            ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
            MapFlags::MAP_SHARED | MapFlags::MAP_NORESERVE | MapFlags::MAP_FIXED,
            fd,
            0,
        )?
    };

    assert_eq!(pa, aligned_begin as _);

    Ok(MR {
        addr: addr as _,
        len,
        memfd: fd,
        shm_file: shm_file.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use nix::unistd::lseek;
    use nix::unistd::Whence;
    use std::slice;
    use std::thread;
    use std::time::Duration;

    use super::*;

    fn posix_memalign<T>(align: usize, len: usize) -> Result<*mut T, nix::Error> {
        let mut addr = ptr::null_mut();
        let err = unsafe { libc::posix_memalign(&mut addr as *mut _ as _, align, len) };
        if err != 0 {
            return Err(nix::Error::from_i32(err));
        }
        Ok(addr)
    }

    #[test]
    fn test_reg_mr_unaligned() -> Result<(), Box<dyn std::error::Error>> {
        let len = 1024 + 4096 + 1024;
        let align = 1024;
        let text = b"test_reg_mr_unaligned";

        let child = unsafe { libc::fork() };
        if child != 0 {
            // parent process
            let mut addr: *mut u8;
            loop {
                addr = posix_memalign(align, len)?;
                if addr as usize % 4096 + 1024 == 4096 {
                    break;
                }
                unsafe {
                    libc::free(addr as _);
                }
            }
            thread::sleep(Duration::from_secs(1));
            let mr = reg_mr(addr, len, SHM_FILE2)?;
            // send_fd("/tmp/testsock1", mr.memfd)?;
            unsafe {
                ptr::copy_nonoverlapping(text.as_ptr(), mr.addr, text.len());
            }
            thread::sleep(Duration::from_secs(2));
            unsafe {
                libc::free(addr as _);
            }
        } else {
            // child process
            // let fd = recv_fd("/tmp/testsock1")?;
            thread::sleep(Duration::from_secs(2));

             let fd = shm_open(
                 SHM_FILE2,
                 OFlag::O_CREAT | OFlag::O_RDWR,
                 Mode::S_IRUSR | Mode::S_IWUSR,
             )?;

            let shm_len = lseek(fd, 0, Whence::SeekEnd)? as usize;
            assert_eq!(shm_len, 4096 * 3);

            let pa = unsafe {
                mmap(
                    ptr::null_mut(),
                    4096 * 3,
                    ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                    MapFlags::MAP_SHARED | MapFlags::MAP_NORESERVE,
                    fd,
                    0,
                )?
            };

            let offset = 4096 - 1024;

            let data =
                unsafe { slice::from_raw_parts((pa as *const u8).offset(offset), text.len()) };
            assert_eq!(data, text);

            unsafe {
                let _ = munmap(pa, len);
                let _ = close(fd);
            }
        }

        Ok(())
    }

    #[test]
    fn test_reg_mr_aligned() -> Result<(), nix::Error> {
        let len = 4096;
        let align = 4096;
        let text = b"test_reg_mr_aligned";

        let child = unsafe { libc::fork() };
        if child != 0 {
            // parent process
            let addr: *mut u8 = posix_memalign(align, len)?;
            let mr = reg_mr(addr, len, SHM_FILE)?;
            unsafe {
                ptr::copy_nonoverlapping(text.as_ptr(), mr.addr, text.len());
            }
            thread::sleep(Duration::from_secs(2));
            unsafe {
                libc::free(addr as _);
            }
        } else {
            // child process
            thread::sleep(Duration::from_secs(1));

            let fd = shm_open(
                SHM_FILE,
                OFlag::O_CREAT | OFlag::O_RDWR,
                Mode::S_IRUSR | Mode::S_IWUSR,
            )?;

            let shm_len = lseek(fd, 0, Whence::SeekEnd)? as usize;
            assert_eq!(shm_len, len);

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

            let data = unsafe { slice::from_raw_parts(pa as *const u8, text.len()) };
            assert_eq!(data, text);

            unsafe {
                let _ = munmap(pa, len);
                let _ = close(fd);
            }
        }

        Ok(())
    }
}
