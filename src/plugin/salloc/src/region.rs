//! Shared memory region.

use std::alloc::Layout;
use std::io;
use std::ops::{Deref, DerefMut};
use std::os::unix::prelude::AsRawFd;

use memfd::{Memfd, MemfdOptions};
use mmap::MmapFixed;
use thiserror::Error;

use uapi::{AsHandle, Handle};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Memfd: {0}.")]
    Memfd(#[from] memfd::Error),
    #[error("IO: {0}.")]
    Io(#[from] io::Error),
}

#[derive(Debug)]
pub struct SharedRegion {
    mmap: MmapFixed,
    memfd: Memfd,
    align: usize,
}

impl Deref for SharedRegion {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.mmap[..]
    }
}

impl DerefMut for SharedRegion {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.mmap[..]
    }
}

impl AsHandle for SharedRegion {
    #[inline]
    fn as_handle(&self) -> Handle {
        Handle(self.memfd.as_raw_fd() as _)
    }
}

impl SharedRegion {
    pub fn new(layout: Layout, addr_mediator: &AddressMediator) -> Result<Self, Error> {
        let nbytes = layout.size();
        let align = layout.align().max(page_size());
        let hugetlb_size = None;

        let opts = MemfdOptions::default()
            .allow_sealing(true)
            .close_on_exec(false)
            .hugetlb(hugetlb_size);

        let name = format!("shared-mr-{}", nbytes);
        let memfd = opts.create(name)?;
        memfd.as_file().set_len(nbytes as u64)?;

        let target_addr = addr_mediator.allocate(layout);
        let mmap = MmapFixed::new(target_addr, nbytes, 0, memfd.as_file())?;
        Ok(Self { mmap, memfd, align })
    }

    #[inline]
    pub fn memfd(&self) -> &Memfd {
        &self.memfd
    }

    #[inline]
    pub fn align(&self) -> usize {
        self.align
    }
}

impl AsRef<SharedRegion> for SharedRegion {
    fn as_ref(&self) -> &SharedRegion {
        self
    }
}

/// The backend and user applications are forced to mmap the shared memory to the same location.
/// This single-address-space approach avoids the problem of invalid pointers on shared memory.
///
/// `AddressMediator` is used to find an unused address in both address space. In this prototype,
/// since 48bit virtual address space is embrassingly large, we just take the address starting from
/// 0x600000000000 and bump it on each allocation.
///
/// Similar to memory allocation, it takes an `Layout` as input and returns an address that follows
/// the alignment requirement.
pub struct AddressMediator {
    current: spin::Mutex<usize>,
}

impl AddressMediator {
    const STARTING_ADDRESS: usize = 0x600000000000;

    pub(crate) fn new() -> Self {
        Self {
            current: spin::Mutex::new(Self::STARTING_ADDRESS),
        }
    }

    pub(crate) fn allocate(&self, layout: Layout) -> usize {
        let mut current = self.current.lock();
        let next = current.next_multiple_of(layout.align());
        *current = next + layout.size();
        next
    }
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
