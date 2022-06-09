use std::ops::{Deref, DerefMut};
use std::slice;

use memfd::Memfd;
use memmap_fixed::MmapFixed;

use ipc::salloc::cmd::{Command, CompletionKind};
use libkoala::_rx_recv_impl as rx_recv_impl;

use super::{Error, SA_CTX};

// Shared recv buffer
#[derive(Debug)]
pub(crate) struct SharedRecvBuffer {
    mmap: MmapFixed,
    remote_addr: usize,
    _memfd: Memfd,
}

impl Deref for SharedRecvBuffer {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.mmap.as_ptr().cast(), self.mmap.len()) }
    }
}

impl DerefMut for SharedRecvBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { slice::from_raw_parts_mut(self.mmap.as_mut_ptr().cast(), self.mmap.len()) }
    }
}

impl SharedRecvBuffer {
    pub(crate) fn new(
        remote_addr: usize,
        nbytes: usize,
        file_off: i64,
        memfd: Memfd,
    ) -> Result<Self, Error> {
        tracing::debug!("SharedRecvBuffer::new, remote_addr: {:#0x?}", remote_addr);

        // Map to the same address as remote_addr, panic if it does not work
        let mmap = MmapFixed::new(remote_addr, nbytes, file_off as i64, memfd.as_file())?;

        // NOTE(wyj): align is not needed for shared recv buffer
        // as we don't need to query backend addr for shared recv buffer
        Ok(SharedRecvBuffer {
            mmap,
            remote_addr,
            _memfd: memfd,
        })
    }
}

// Shared region on sender heap
#[derive(Debug)]
pub(crate) struct SharedHeapRegion {
    mmap: MmapFixed,
    remote_addr: usize,
    pub(crate) align: usize,
    _memfd: Memfd,
}

impl Deref for SharedHeapRegion {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.mmap.as_ptr().cast(), self.mmap.len()) }
    }
}

impl DerefMut for SharedHeapRegion {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { slice::from_raw_parts_mut(self.mmap.as_mut_ptr().cast(), self.mmap.len()) }
    }
}

impl Drop for SharedHeapRegion {
    fn drop(&mut self) {
        (|| {
            SA_CTX.with(|ctx| {
                let req = Command::DeallocShm(self.remote_addr);
                ctx.service.send_cmd(req)?;
                // TODO(wyj): do we really need to wait for completion here?
                rx_recv_impl!(ctx.service, CompletionKind::DeallocShm)
            })
        })()
        .unwrap_or_else(|e| eprintln!("Dropping SharedHeapRegion: {}", e));
    }
}

impl SharedHeapRegion {
    pub(crate) fn new(
        remote_addr: usize,
        nbytes: usize,
        align: usize,
        file_off: i64,
        memfd: Memfd,
    ) -> Result<Self, Error> {
        tracing::debug!("SharedHeapRegion::new, remote_addr: {:#0x?}", remote_addr);

        // Map to the same address as remote_addr, panic if it does not work
        let mmap = MmapFixed::new(remote_addr, nbytes, file_off as i64, memfd.as_file())?;

        Ok(SharedHeapRegion {
            mmap,
            remote_addr,
            align,
            _memfd: memfd,
        })
    }

    #[inline]
    pub(crate) fn as_ptr(&self) -> *const u8 {
        self.mmap.as_ptr()
    }

    #[inline]
    pub(crate) fn as_mut_ptr(&self) -> *mut u8 {
        self.mmap.as_mut_ptr()
    }

    #[inline]
    pub(crate) fn remote_addr(&self) -> usize {
        self.remote_addr
    }
}
