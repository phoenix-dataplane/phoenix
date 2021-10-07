use std::fs::{self, File};
use std::marker::PhantomData;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::net::{AncillaryData, SocketAncillary, UnixDatagram};
use std::path::Path;
use std::ptr;
use std::slice;
use std::thread;
use std::time::Duration;

use nix::fcntl::OFlag;
use nix::sys::mman::{mmap, msync, munmap, shm_open, shm_unlink, MapFlags, MsFlags, ProtFlags};
use nix::sys::stat::Mode;
use nix::unistd::{sysconf, SysconfVar};

pub struct SMR<'a> {
    buffer: &'static [u8],
    memfd: File,
    shm_file: String,
    _marker: PhantomData<&'a u8>,
}

impl<'a> SMR<'a> {
    #[inline]
    pub fn page_aligned_begin(&self) -> *const u8 {
        let page_size = *PAGE_SIZE;
        let addr = self.buffer.as_ptr() as usize;
        (addr - addr % page_size) as _
    }
    #[inline]
    pub fn page_aligned_end(&self) -> *const u8 {
        let page_size = *PAGE_SIZE;
        let addr = self.buffer.as_ptr() as usize;
        let len = self.buffer.len();
        ((addr + len + page_size - 1) / page_size * page_size) as _
    }
    #[inline]
    pub fn page_aligned_len(&self) -> usize {
        unsafe {
            // safety: the result won't be out range in our case
            self.page_aligned_end()
                .offset_from(self.page_aligned_begin()) as _
        }
    }

    pub fn new(buffer: &[u8], shm_file: &str) -> Result<SMR<'a>, Box<dyn std::error::Error>> {
        let shm_path = Path::new("/dev/shm").join(Path::new(shm_file));
        if shm_path.exists() {
            fs::remove_file(shm_path)?;
        }
        let fd = shm_open(
            shm_file,
            OFlag::O_CREAT | OFlag::O_RDWR | OFlag::O_EXCL,
            Mode::S_IRUSR | Mode::S_IWUSR,
        )?;
        let memfile = unsafe { File::from_raw_fd(fd) };

        let addr = buffer.as_ptr();
        let len = buffer.len();

        let page_size = get_page_size();
        let aligned_end = (addr as usize + len + page_size - 1) / page_size * page_size;
        let aligned_begin = addr as usize - addr as usize % page_size;
        let aligned_len = aligned_end - aligned_begin;

        memfile.set_len(aligned_len as _)?;

        let old_pa = unsafe {
            mmap(
                ptr::null_mut(),
                aligned_len,
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED | MapFlags::MAP_NORESERVE,
                memfile.as_raw_fd(),
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
                memfile.as_raw_fd(),
                0,
            )?
        };

        assert_eq!(pa, aligned_begin as _);

        Ok(SMR {
            buffer: unsafe { slice::from_raw_parts(buffer.as_ptr(), buffer.len()) },
            memfd: memfile,
            shm_file: shm_file.to_owned(),
            _marker: PhantomData,
        })
    }
}

impl<'a> Drop for SMR<'a> {
    fn drop(&mut self) {
        // recovery the pages right after performing munmap
        use std::io::{Read, Seek, SeekFrom};
        if let Ok(_) = self.memfd.seek(SeekFrom::Start(0)) {
            unsafe {
                // it is safe to call msync, but I'm not quite sure if this is actually necessary
                msync(
                    self.page_aligned_begin() as _,
                    self.page_aligned_len(),
                    MsFlags::MS_SYNC,
                )
                .unwrap();

                // unmap the shared pages
                munmap(self.page_aligned_begin() as _, self.page_aligned_len()).unwrap();

                // how to query the prot flags of the original pages?
                // if we can query at the beginning, we should use those.
                let new_pa = mmap(
                    self.page_aligned_begin() as _,
                    self.page_aligned_len(),
                    ProtFlags::PROT_READ
                        | ProtFlags::PROT_WRITE
                        | ProtFlags::PROT_GROWSUP
                        | ProtFlags::PROT_GROWSDOWN,
                    MapFlags::MAP_PRIVATE | MapFlags::MAP_ANONYMOUS | MapFlags::MAP_FIXED,
                    -1,
                    0,
                )
                .unwrap();

                assert_eq!(new_pa, self.page_aligned_begin() as _);
                let mut orig_buffer = slice::from_raw_parts_mut(
                    self.page_aligned_begin() as _,
                    self.page_aligned_len(),
                );
                self.memfd
                    .read_exact(&mut orig_buffer)
                    .unwrap_or_else(|e| panic!("read_exact: {}", e));
            }
        }

        let _ = shm_unlink(self.shm_file.as_str());
    }
}

lazy_static::lazy_static! {
    static ref PAGE_SIZE: usize = get_page_size();
}

#[allow(dead_code)]
fn get_page_size() -> usize {
    sysconf(SysconfVar::PAGE_SIZE).unwrap().unwrap() as _
}

#[allow(dead_code)]
pub fn send_fd<P: AsRef<Path>>(sock_path: P, fd: RawFd) -> Result<(), std::io::Error> {
    let sock = UnixDatagram::unbound()?;
    // connect retry for 1 seconds
    let mut retry_cnt = 0;
    while let Err(e) = sock.connect(&sock_path) {
        thread::sleep(Duration::from_millis(100));
        retry_cnt += 1;
        if retry_cnt > 10 {
            return Err(e);
        }
    }

    let mut ancillary_buffer = [0; 128];
    let mut ancillary = SocketAncillary::new(&mut ancillary_buffer[..]);
    ancillary.add_fds(&[fd][..]);

    let bufs = &mut [][..];
    sock.send_vectored_with_ancillary(bufs, &mut ancillary)?;
    Ok(())
}

#[allow(dead_code)]
pub fn recv_fd<P: AsRef<Path>>(sock_path: P) -> Result<RawFd, std::io::Error> {
    if sock_path.as_ref().exists() {
        fs::remove_file(&sock_path)?;
    }
    let sock = UnixDatagram::bind(&sock_path)?;
    let bufs = &mut [][..];
    let mut fds = Vec::new();
    let mut ancillary_buffer = [0; 128];
    let mut ancillary = SocketAncillary::new(&mut ancillary_buffer[..]);
    let (_size, truncated) = sock.recv_vectored_with_ancillary(bufs, &mut ancillary)?;

    assert!(!truncated);
    for ancillary_result in ancillary.messages() {
        if let AncillaryData::ScmRights(scm_rights) = ancillary_result.unwrap() {
            for fd in scm_rights {
                fds.push(fd);
            }
        }
    }
    assert_eq!(fds.len(), 1);

    fs::remove_file(sock_path)?;
    Ok(fds[0])
}

#[cfg(test)]
mod tests {
    use nix::sys::wait::{waitpid, WaitStatus};
    use nix::unistd::{fork, ForkResult};
    use std::slice;

    use super::*;
    use aligned_region::AlignedRegion;

    const SHM_FILE: &'static str = "testpage_aligned";
    const SHM_FILE2: &'static str = "testpage_unaligned";

    mod aligned_region {
        use std::mem::ManuallyDrop;
        use std::ops::{Deref, DerefMut};
        use std::ptr;

        fn posix_memalign<T>(align: usize, len: usize) -> Result<*mut T, nix::Error> {
            let mut addr = ptr::null_mut();
            let err = unsafe { libc::posix_memalign(&mut addr as *mut _ as _, align, len) };
            if err != 0 {
                return Err(nix::Error::from_i32(err));
            }
            Ok(addr)
        }

        pub struct AlignedRegion<T: Sized> {
            data: ManuallyDrop<Vec<T>>,
        }

        impl<T: Sized> Drop for AlignedRegion<T> {
            fn drop(&mut self) {
                unsafe {
                    libc::free(self.data.as_ptr() as _);
                }
            }
        }

        impl<T: Sized> Deref for AlignedRegion<T> {
            type Target = [T];
            fn deref(&self) -> &Self::Target {
                self.data.as_slice()
            }
        }

        impl<T: Sized> DerefMut for AlignedRegion<T> {
            fn deref_mut(&mut self) -> &mut Self::Target {
                self.data.as_mut_slice()
            }
        }

        impl<T: Sized> AlignedRegion<T> {
            pub fn new(align: usize, len: usize) -> Result<AlignedRegion<T>, nix::Error> {
                assert!(len > 0);
                let addr = posix_memalign(align, len)?;
                Ok(Self {
                    data: ManuallyDrop::new(unsafe { Vec::from_raw_parts(addr, len, len) }),
                })
            }

            pub fn as_ptr<V>(&self) -> *const V {
                self.data.as_ptr() as _
            }

            pub fn as_mut_ptr<V>(&mut self) -> *mut V {
                self.data.as_mut_ptr() as _
            }

            pub fn len(&self) -> usize {
                self.data.len()
            }
        }
    }

    #[test]
    fn test_reg_mr_unaligned() -> Result<(), Box<dyn std::error::Error>> {
        let len = 1024 + 4096 + 1024;
        let align = 1024;
        let text = b"test_reg_mr_unaligned";

        match unsafe { fork() } {
            Ok(ForkResult::Parent { child }) => {
                // parent process
                // retry allocate until finding an allocation of special alignment
                let mut ar: AlignedRegion<u8>;
                let mut retry_cnt = 0;
                loop {
                    ar = AlignedRegion::new(align, len)?;
                    let addr = ar.as_ptr::<u8>() as usize;
                    if addr as usize % 4096 + 1024 == 4096 {
                        break;
                    }
                    retry_cnt += 1;
                    if retry_cnt > 100 {
                        return Err("Cannot find an allocation with satisfied alignment".into());
                    }
                }

                let mr = SMR::new(&ar, SHM_FILE2)?;

                // write something
                unsafe {
                    ptr::copy_nonoverlapping(text.as_ptr(), ar.as_mut_ptr(), text.len());
                }

                send_fd("/tmp/testsock_unaligned", mr.memfd.as_raw_fd())?;

                // waitpid
                let status = waitpid(child, None)?;
                assert!(matches!(status, WaitStatus::Exited(..)));
            }
            Ok(ForkResult::Child) => {
                let fd = recv_fd("/tmp/testsock_unaligned")?;

                let memfile = unsafe { File::from_raw_fd(fd) };
                let shm_len = memfile.metadata()?.len() as usize;

                assert_eq!(shm_len, 4096 * 3);

                let pa = unsafe {
                    mmap(
                        ptr::null_mut(),
                        4096 * 3,
                        ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                        MapFlags::MAP_SHARED | MapFlags::MAP_NORESERVE,
                        memfile.as_raw_fd(),
                        0,
                    )?
                };

                let offset = 4096 - 1024;

                let data =
                    unsafe { slice::from_raw_parts((pa as *const u8).offset(offset), text.len()) };
                assert_eq!(data, text);

                unsafe {
                    let _ = munmap(pa, len);
                }
            }
            Err(e) => panic!("fork failed: {}", e),
        }

        Ok(())
    }

    #[test]
    fn test_reg_mr_aligned() -> Result<(), Box<dyn std::error::Error>> {
        let len = 4096;
        let align = 4096;
        let text = b"test_reg_mr_aligned";

        match unsafe { fork() } {
            Ok(ForkResult::Parent { child }) => {
                // parent process
                let mut ar: AlignedRegion<u8> = AlignedRegion::new(align, len)?;
                let mr = SMR::new(&ar, SHM_FILE)?;

                // write something to the memory
                unsafe {
                    ptr::copy_nonoverlapping(text.as_ptr(), ar.as_mut_ptr(), text.len());
                }

                // send the file descriptor
                send_fd("/tmp/testsock_aligned", mr.memfd.as_raw_fd())?;

                // waitpid
                let status = waitpid(child, None)?;
                assert!(matches!(status, WaitStatus::Exited(..)));
            }
            Ok(ForkResult::Child) => {
                // child process
                let fd = recv_fd("/tmp/testsock_aligned")?;

                let memfile = unsafe { File::from_raw_fd(fd) };
                let shm_len = memfile.metadata()?.len() as usize;
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
                }
            }
            Err(e) => panic!("fork failed: {}", e),
        }

        Ok(())
    }
}
