//! Read-only shared memory heap.
use std::io;
use std::ops::Deref;
use std::os::unix::io::RawFd;
use std::slice;
use std::sync::atomic::{AtomicUsize, Ordering};

use memfd::Memfd;
use mmap::MmapFixed;

use phoenix_api::{AsHandle, Handle};
use phoenix_api_mrpc::cmd::ConnectResponse;

use super::Error;

/// A collection of read-only memory-mapped regions that are guarded by the same reference counter
/// (the regions are bounded to the same connection).
#[derive(Debug)]
pub struct ReadHeap {
    /// The number of [`RRef<T>`](crate::rref::RRef<T>)s pointing to this heap.
    pub(crate) rref_cnt: AtomicUsize,
    pub(crate) rbufs: Vec<ReadRegion>,
}

impl Drop for ReadHeap {
    fn drop(&mut self) {
        let cnt = self.rref_cnt.load(Ordering::Acquire);
        assert_eq!(
            cnt, 0,
            "Found {} outstanding references still pointing to this heap",
            cnt
        );
    }
}

impl ReadHeap {
    /// Creates a [`ReadHeap`] from a connection establishment event and a list of memfds.
    pub fn new(conn_resp: &ConnectResponse, fds: &[RawFd]) -> Self {
        let mut rbufs = Vec::new();
        for (rbuf, &fd) in conn_resp.read_regions.iter().zip(fds) {
            let memfd = Memfd::try_from_fd(fd)
                .map_err(|_| io::Error::last_os_error())
                .unwrap();
            let m =
                ReadRegion::new(rbuf.handle, rbuf.addr, rbuf.len, rbuf.file_off, memfd).unwrap();
            // vaddrs.push((mr.0, m.as_ptr().expose_addr()));
            rbufs.push(m);
        }
        ReadHeap {
            rref_cnt: AtomicUsize::new(0),
            rbufs,
        }
    }

    pub fn default() -> Self {
        ReadHeap {
            rref_cnt: AtomicUsize::new(0),
            rbufs: Vec::new(),
        }
    }

    #[inline]
    pub(crate) fn increment_refcnt(&self) {
        self.rref_cnt.fetch_add(1, Ordering::Release);
    }

    #[inline]
    pub(crate) fn decrement_refcnt(&self) {
        self.rref_cnt.fetch_sub(1, Ordering::Release);
    }
}

/// A continuous region shared with the backend service.
///
/// Initially created by the backend service. Later memory-mapped as read-only in the library's
/// address space.
///
/// On destruction, the internal `memfd` will be automatically released.
#[derive(Debug)]
pub(crate) struct ReadRegion {
    mmap: MmapFixed,
    handle: Handle,
    _remote_addr: usize,
    _memfd: Memfd,
}

impl AsHandle for ReadRegion {
    #[inline]
    fn as_handle(&self) -> Handle {
        self.handle
    }
}

impl Deref for ReadRegion {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.mmap.as_ptr().cast(), self.mmap.len()) }
    }
}

impl ReadRegion {
    pub(crate) fn new(
        handle: Handle,
        remote_addr: usize,
        nbytes: usize,
        file_off: i64,
        memfd: Memfd,
    ) -> Result<Self, Error> {
        tracing::trace!("ReadRegion::new, remote_addr: {:#0x?}", remote_addr);

        // Map to the same address as remote_addr, panic if it does not work
        let mmap = MmapFixed::new(remote_addr, nbytes, file_off as i64, memfd.as_file())?;

        // NOTE(wyj): align is not needed for shared recv buffer
        // as we don't need to query backend addr for shared recv buffer
        Ok(ReadRegion {
            mmap,
            handle,
            _remote_addr: remote_addr,
            _memfd: memfd,
        })
    }
}
