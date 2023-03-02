use std::cmp::{max, min};
use std::io;
use std::sync::atomic::{AtomicU32, Ordering};

use memfd::{Memfd, MemfdOptions};
use memmap_fixed::MmapFixed;
use thiserror::Error;

use phoenix_api::buf::Range;
use phoenix_api::{AsHandle, Handle};

static GLOBAL_ID_COUNTER: AtomicU32 = AtomicU32::new(0);

#[derive(Debug, Error)]
pub enum Error {
    #[error("Memfd: {0}.")]
    Memfd(#[from] memfd::Error),
    #[error("IO: {0}.")]
    Io(#[from] io::Error),
}

pub(crate) struct MemoryRegion {
    handle: Handle,
    mmap: MmapFixed,
    memfd: Memfd,
}

impl AsHandle for MemoryRegion {
    #[inline]
    fn as_handle(&self) -> Handle {
        self.handle
    }
}

impl MemoryRegion {
    pub(crate) fn new<'ctx>(nbytes: usize) -> Result<Self, Error> {
        let hugetlb_size = None;

        let opts = MemfdOptions::default()
            .allow_sealing(true)
            .close_on_exec(false)
            .hugetlb(hugetlb_size);
        let name = format!("shared-mr-{}", nbytes);
        let memfd = opts.create(name)?;
        memfd.as_file().set_len(nbytes as u64)?;

        // let align = nbytes
        //     .checked_next_power_of_two()
        //     .expect("next_power_of_two: {len}")
        //     .max(page_size());
        // let layout = Layout::from_size_align(nbytes, align).unwrap();
        let mmap = MmapFixed::new(0, nbytes, 0, memfd.as_file())?;

        let handle = Handle(GLOBAL_ID_COUNTER.fetch_add(1, Ordering::Relaxed));

        Ok(Self {
            handle,
            mmap,
            memfd,
        })
    }

    pub(crate) fn as_slice(&self, mut range: Range) -> &[u8] {
        range.offset = min(max(0, range.offset), self.mmap.len() as _); //Should not use mmap.len()-1
        range.len = min(self.mmap.len() as u64 - range.offset, range.len);
        unsafe {
            std::slice::from_raw_parts(self.mmap.as_ptr().offset(range.offset as _), range.len as _)
        }
    }

    pub(crate) fn as_mut_slice(&self, mut range: Range) -> &mut [u8] {
        range.offset = min(max(0, range.offset), self.mmap.len() as _); //Should not use mmap.len()-1
        range.len = min(self.mmap.len() as u64 - range.offset, range.len);
        unsafe {
            std::slice::from_raw_parts_mut(
                self.mmap.as_mut_ptr().offset(range.offset as _),
                range.len as _,
            )
        }
    }
}
