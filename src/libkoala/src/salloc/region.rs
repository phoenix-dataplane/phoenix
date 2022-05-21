use std::ops::{Deref, DerefMut};
use std::slice;

use memfd::Memfd;
use memmap_fixed::MmapFixed;

use ipc::salloc::cmd::{Command, CompletionKind};

use super::{Error, SA_CTX};
use crate::rx_recv_impl;

#[derive(Debug)]
pub(crate) struct SharedRegion {
    mmap: MmapFixed,
    remote_addr: usize,
    pub(crate) align: usize,
    _memfd: Memfd,
}

impl Deref for SharedRegion {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        unsafe { slice::from_raw_parts(self.mmap.as_ptr().cast(), self.mmap.len()) }
    }
}

impl DerefMut for SharedRegion {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { slice::from_raw_parts_mut(self.mmap.as_mut_ptr().cast(), self.mmap.len()) }
    }
}

impl Drop for SharedRegion {
    fn drop(&mut self) {
        (|| {
            SA_CTX.with(|ctx| {
                let req = Command::DeallocShm(self.remote_addr);
                ctx.service.send_cmd(req)?;
                rx_recv_impl!(ctx.service, CompletionKind::DeallocShm)
            })
        })()
        .unwrap_or_else(|e| eprintln!("Dropping SharedRegion: {}", e));
    }
}

impl SharedRegion {
    pub(crate) fn new(
        remote_addr: usize,
        nbytes: usize,
        align: usize,
        file_off: i64,
        memfd: Memfd,
    ) -> Result<Self, Error> {
        tracing::debug!("SharedRegion::new, remote_addr: {:#0x?}", remote_addr);

        // Map to the same address as remote_addr, panic if it does not work
        let mmap = MmapFixed::new(remote_addr, nbytes, file_off as i64, memfd.as_file())?;

        Ok(SharedRegion {
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
